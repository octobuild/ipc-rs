//! Bindings to System V semaphores
//!
//! When dealing with unix, there are generally two kinds of IPC semaphores, one
//! is the System V semaphore while the other is a POSIX semaphore. The POSIX
//! semaphore is generally easier to use, but it does not relinquish resources
//! when a process terminates unexpectedly. On the other ahnd a System V
//! semaphore provides the option to do so, so the choice was made to use a
//! System V semaphore rather than a POSIX semaphore.
//!
//! System V semaphores are interesting in that they have an unusual
//! initialization procedure where a semaphore is created and *then*
//! initialized. As in, these two steps are not atomic. This causes some
//! confusion down below, as you'll see in `fn new`.
//!
//! Additionally all semaphores need a `key_t` which originates from an actual
//! existing file, so this implementation ensures that a file exists when
//! creating a semaphore.

#![allow(bad_style)]

use libc::{sembuf, semid_ds, EEXIST, O_RDWR};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Error, ErrorKind, Result};
use std::mem;
use std::path::PathBuf;

use self::consts::{SEM_UNDO, SETVAL};
use std::collections::hash_map::DefaultHasher;

pub struct Semaphore {
    semid: libc::c_int,
}

#[cfg(target_os = "linux")]
mod consts {
    pub static SEM_UNDO: libc::c_short = 0x1000;
    pub static SETVAL: libc::c_int = 16;
}

#[cfg(target_os = "macos")]
mod consts {
    pub static SEM_UNDO: libc::c_short = libc::SEM_UNDO;
    pub static SETVAL: libc::c_int = libc::SETVAL;
}

impl Semaphore {
    pub unsafe fn new(name: &str, cnt: usize) -> Result<Semaphore> {
        let key = Semaphore::key(name)?;

        // System V semaphores cannot be initialized at creation, and we don't
        // know which process is responsible for creating the semaphore, so we
        // partially assume that we are responsible.
        //
        // In order to get "atomic create and initialization" we have a dirty
        // hack here. First, an attempt is made to exclusively create the
        // semaphore. If we succeed, then we're responsible for initializing it.
        // If we fail, we need to wait for someone's initialization to succeed.
        // We read off the `sem_otime` field in a loop to "wait until a
        // semaphore is initialized." Sadly I don't know of a better way to get
        // around this...
        //
        // see http://beej.us/guide/bgipc/output/html/multipage/semaphores.html
        let mut semid = libc::semget(key, 1, libc::IPC_CREAT | libc::IPC_EXCL | 0o666);
        if semid >= 0 {
            let mut buf = sembuf {
                sem_num: 0,
                sem_op: cnt as libc::c_short,
                sem_flg: 0,
            };
            // Be sure to clamp the value to 0 and then add the necessary count
            // onto it. The clamp is necessary as the initial value seems to be
            // generally undefined, and the bump is then necessary to modify
            // sem_otime.
            if libc::semctl(semid, 0, SETVAL, 0) != 0 || libc::semop(semid, &mut buf, 1) != 0 {
                let err = Error::last_os_error();
                libc::semctl(semid, 0, libc::IPC_RMID);
                return Err(err);
            }
        } else {
            match Error::last_os_error() {
                ref e if e.raw_os_error() == Some(EEXIST) => {
                    // Re-attempt to get the semaphore, this should in theory always
                    // succeed?
                    semid = libc::semget(key, 1, 0);
                    if semid < 0 {
                        return Err(Error::last_os_error());
                    }

                    // Spin in a small loop waiting for sem_otime to become not 0
                    let mut ok = false;
                    for _ in 0..1000 {
                        let mut buf: semid_ds = mem::zeroed();
                        if libc::semctl(semid, 0, libc::IPC_STAT, &mut buf) != 0 {
                            return Err(Error::last_os_error());
                        }
                        if buf.sem_otime != 0 {
                            ok = true;
                            break;
                        }
                    }
                    if !ok {
                        return Err(Error::new(
                            ErrorKind::TimedOut,
                            "timed out waiting for sem to be initialized",
                        ));
                    }
                }
                e => return Err(e),
            }
        }

        // Phew! That took long enough...
        Ok(Semaphore { semid })
    }

    /// Get value hash
    fn hash<T: Hash>(value: &T) -> u64 {
        let mut h = DefaultHasher::new();
        value.hash(&mut h);
        h.finish()
    }

    /// Generate the filename which will be passed to ftok, keyed off the given
    /// semaphore name `name`.
    fn filename(name: &str) -> PathBuf {
        let filename = name
            .chars()
            .filter(|a| (*a as u32) < 128 && a.is_alphanumeric())
            .collect::<String>();
        env::temp_dir().join("ipc-rs-sems").join(format!(
            "{}-{}",
            filename,
            Semaphore::hash::<_>(&(name, "ipc-rs"))
        ))
    }

    /// Generate the `key_t` from `ftok` which will be passed to `semget`.
    ///
    /// This function will ensure that the relevant file is located on the
    /// filesystem and will then invoke ftok on it.
    unsafe fn key(name: &str) -> Result<libc::key_t> {
        let filename = Semaphore::filename(name);
        let dir = filename.parent().unwrap();

        // As long as someone creates the directory we're alright.
        let _ = fs::create_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Make sure that the file exists. Open it in exclusive/create mode to
        // ensure that it's there, but don't overwrite it if it alredy exists.
        //
        // see QSharedMemoryPrivate::createUnixKeyFile in Qt
        let filename = filename.to_str().unwrap().to_string() + "\0";
        let fd = libc::open(
            filename.as_ptr() as *const i8,
            libc::O_EXCL | libc::O_CREAT | O_RDWR,
            0o640,
        );
        if fd > 0 {
            libc::close(fd);
        } else {
            match Error::last_os_error() {
                ref e if e.raw_os_error() == Some(EEXIST) => {}
                e => return Err(e),
            }
        }

        // Invoke `ftok` with our filename
        let key = libc::ftok(filename.as_ptr() as *const libc::c_char, 'I' as libc::c_int);
        if key != -1 {
            Ok(key)
        } else {
            Err(Error::last_os_error())
        }
    }

    pub unsafe fn wait(&self) {
        loop {
            if self.modify(-1, true) == 0 {
                return;
            }

            match Error::last_os_error() {
                ref e if e.raw_os_error() == Some(libc::EINTR) => {}
                e => panic!("unknown wait error: {}", e),
            }
        }
    }

    pub unsafe fn try_wait(&self) -> bool {
        if self.modify(-1, false) == 0 {
            return true;
        }

        match Error::last_os_error() {
            ref e if e.raw_os_error() == Some(libc::EAGAIN) => false,
            e => panic!("unknown try_wait error: {}", e),
        }
    }

    pub unsafe fn post(&self) {
        if self.modify(1, true) == 0 {
            return;
        }
        panic!("unknown post error: {}", Error::last_os_error())
    }

    unsafe fn modify(&self, amt: i16, wait: bool) -> libc::c_int {
        let mut buf = sembuf {
            sem_num: 0,
            sem_op: amt,
            sem_flg: if wait {
                0
            } else {
                libc::IPC_NOWAIT as libc::c_short
            } | SEM_UNDO,
        };
        libc::semop(self.semid, &mut buf, 1)
    }
}

impl Drop for Semaphore {
    fn drop(&mut self) {}
}
