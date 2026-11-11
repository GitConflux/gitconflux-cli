#![allow(clippy::expect_used)]

use std::fs;

use gitconflux_core::git::{Git, resolve_commit};
use gitconflux_core::proposal::{changed_files, create_bundle};

fn commit(git: &Git, msg: &str) {
    git.run(&["add", "-A"]).expect("add");
    git.run(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-q",
        "-m",
        msg,
    ])
    .expect("commit");
}

fn repo() -> (tempfile::TempDir, Git) {
    let dir = tempfile::tempdir().expect("tmp");
    let git = Git::new(dir.path());
    git.run(&["init", "-q", "-b", "main"]).expect("init");
    fs::write(dir.path().join("a.txt"), "one\n").expect("write");
    commit(&git, "base");
    (dir, git)
}

#[test]
fn bundles_and_cleans_up_temp_ref() {
    let (dir, git) = repo();
    let base = resolve_commit(&git, "HEAD").expect("base");
    fs::write(dir.path().join("b.txt"), "two\n").expect("write");
    fs::write(dir.path().join("a.txt"), "changed\n").expect("write");
    commit(&git, "head");
    let head = resolve_commit(&git, "HEAD").expect("head");

    let bundle = create_bundle(&git, &base, &head).expect("bundle");
    assert_eq!(bundle.sha256.len(), 64);
    assert_eq!(bundle.size, fs::metadata(&bundle.path).expect("meta").len());
    assert!(
        git.run(&["bundle", "verify", bundle.path.to_str().expect("utf8")])
            .is_ok()
    );

    let refs = git
        .run(&["for-each-ref", "refs/gitconflux/"])
        .expect("refs");
    assert_eq!(refs.stdout, Vec::<u8>::new(), "temp ref must be removed");

    let files = changed_files(&git, &base, &head).expect("files");
    assert_eq!(files.len(), 2);
    assert!(
        files
            .iter()
            .any(|f| f.path == "b.txt" && f.status == "added")
    );
}

#[test]
fn rejects_non_ancestor_base() {
    let (dir, git) = repo();
    git.run(&["checkout", "-q", "-b", "other"]).expect("branch");
    fs::write(dir.path().join("x.txt"), "x\n").expect("write");
    commit(&git, "other");
    let other = resolve_commit(&git, "HEAD").expect("other");
    git.run(&["checkout", "-q", "main"]).expect("main");
    fs::write(dir.path().join("y.txt"), "y\n").expect("write");
    commit(&git, "main2");
    let main2 = resolve_commit(&git, "HEAD").expect("main2");
    assert!(create_bundle(&git, &other, &main2).is_err());
    let refs = git
        .run(&["for-each-ref", "refs/gitconflux/"])
        .expect("refs");
    assert_eq!(refs.stdout, Vec::<u8>::new());
}

#[test]
fn bad_revision_is_an_error() {
    let (_dir, git) = repo();
    assert!(resolve_commit(&git, "does-not-exist").is_err());
    assert!(resolve_commit(&git, "--all").is_err());
}
