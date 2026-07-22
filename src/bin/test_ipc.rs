//! Subprocess entry point for IPC semaphore integration tests.
//!
//! This binary is spawned by the integration tests to exercise cross-process
//! semaphore communication. It is NOT compiled with the test harness.

use std::process::Command;

fn me() -> Command {
    let mut me = Command::new(std::env::current_exe().unwrap());
    me.env("IPC_TEST_INNER", "1");
    me
}

fn first_pass() {
    let sem1 = ipc::Semaphore::new("foo1", 1).unwrap();
    let sem2 = ipc::Semaphore::new("foo2", 0).unwrap();
    println!("[0] Lock foo1");
    let g1 = sem1.access();
    println!("[0] Start");
    let mut p = me().arg("test1_inner").spawn().unwrap();
    sem2.acquire();
    println!("[0] Lock foo2");
    println!("[0] Unlock foo1");
    drop(g1);
    p.wait().unwrap();
    println!("[0] Join");
    drop(sem1.access());
}

fn inner_pass() {
    let sem1 = ipc::Semaphore::new("foo1", 0).unwrap();
    let sem2 = ipc::Semaphore::new("foo2", 0).unwrap();
    println!("[1] Unlock foo2");
    sem2.release();
    let _ = sem1.access();
    println!("[1] Lock foo1");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        return;
    }
    for arg in &args[1..] {
        println!("Enter: {arg}");
        match arg.as_str() {
            "test1_inner" => inner_pass(),
            "test1" => first_pass(),
            v => panic!("Unknown test: {}", v),
        }
        println!("Leave: {arg}");
    }
}
