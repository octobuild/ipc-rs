use std::process::Command;

#[test]
fn test1() {
    let output = Command::new("cargo")
        .args(["run", "--bin", "test_ipc", "--", "test1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        std::str::from_utf8(&output.stdout).unwrap(),
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
