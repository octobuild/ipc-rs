#![reexport_test_harness_main = "test_main"]

extern crate ipc;

use std::env;
use std::process::Command;
use std::str;

fn main() {
    let mut args = env::args();
    if args.len() == 1 {
        return run_test();
    }
    args.next().unwrap();
    for arg in args {
        println!("Enter: {}", arg);
        match &arg as &str {
            "test1_inner" => {
                let sem1 = ipc::Semaphore::new("foo1", 0).unwrap();
                let sem2 = ipc::Semaphore::new("foo2", 0).unwrap();
                println!("[1] Unlock foo2");
                sem2.release();
                let _ = sem1.access();
                println!("[1] Lock foo1");
            }
            "test1" => first_pass(),
            v => panic!("Unknown test: {}", v),
        }
        println!("Leave: {}", arg);
    }
}

fn me() -> Command {
    Command::new(env::current_exe().unwrap())
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

fn run_test() {
    let test_exe = env::current_exe().unwrap();
    let output = Command::new(test_exe).arg("test1").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        str::from_utf8(&output.stdout).unwrap(),
        r#"Enter: test1
[0] Lock foo1
[0] Start
Enter: test1_inner
[1] Unlock foo2
[0] Lock foo2
[0] Unlock foo1
[1] Lock foo1
Leave: test1_inner
[0] Join
Leave: test1
"#
    );
}
