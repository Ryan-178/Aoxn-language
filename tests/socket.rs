//! stdlib/socket.ax loopback tests. Windows uses Winsock (-l ws2_32);
//! Linux uses POSIX sockets. The tests run real TCP connections on both
//! platforms — cross-compiling is not evidence.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

const SERVER: &str = r#"
import * from "stdlib/socket.ax"


def parse_port(s: string) -> int:
    v = 0
    i = 0
    while i < len(s):
        c = str_get(s, i)
        if c < 48 or c > 57:
            return -1
        v = v * 10 + c - 48
        i = i + 1
    return v

def main() -> int:
    port = parse_port(arg(0))
    ls = socket_listen(port, 4)
    if ls.err != 0:
        print("ERR " + ls.msg)
        return 1
    cs = socket_accept(ls)
    if cs.err != 0:
        print("ERR " + cs.msg)
        socket_close(ls)
        return 1
    buf = malloc(512)
    n = socket_recv_line(cs.fd, buf, 512)
    if n < 0:
        print("ERR recv")
        socket_close(cs)
        socket_close(ls)
        return 1
    line = as_string(buf)
    socket_send_str(cs.fd, "echo:" + line + "\n")
    socket_close(cs)
    socket_close(ls)
    print("server:" + line)
    return 0
"#;

const CLIENT: &str = r#"
import * from "stdlib/socket.ax"


def parse_port(s: string) -> int:
    v = 0
    i = 0
    while i < len(s):
        c = str_get(s, i)
        if c < 48 or c > 57:
            return -1
        v = v * 10 + c - 48
        i = i + 1
    return v

def main() -> int:
    port = parse_port(arg(0))
    s = socket_connect("127.0.0.1", port)
    if s.err != 0:
        print("ERR " + s.msg)
        return 1
    socket_send_str(s.fd, "ping\n")
    buf = malloc(256)
    n = socket_recv_line(s.fd, buf, 256)
    socket_close(s)
    if n < 0:
        print("ERR recv")
        return 1
    print("client:" + as_string(buf))
    return 0
"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build(dir: &Path, name: &str, src: &str) -> PathBuf {
    let path = dir.join(format!("{name}.ax"));
    fs::write(&path, src).unwrap();
    let exe = dir.join(format!("{name}{EXE}"));
    let libs: Vec<String> = if cfg!(windows) {
        vec!["ws2_32".to_string()]
    } else {
        Vec::new()
    };
    aoxn::build_paths_opts(&[path.display().to_string()], &exe, true, &libs, &[])
        .unwrap_or_else(|d| panic!("{name} failed to compile: {d:?}"));
    exe
}

fn pick_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn wait_exit(child: &mut Child, limit: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            panic!("child did not exit in time");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_timeout(cmd: &mut Command, limit: Duration) -> std::process::Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            panic!("command did not exit in time");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn connect_retry(port: u16) -> TcpStream {
    let start = Instant::now();
    loop {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            return s;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "server never listened");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn loopback_roundtrip_both_directions_and_errors() {
    let Some(_clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "clang required"
        );
        eprintln!("skipping: clang unavailable");
        return;
    };
    let dir = root().join("target").join(format!("socket-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let server_exe = build(&dir, "echo_server", SERVER);
    let client_exe = build(&dir, "echo_client", CLIENT);

    // Aoxn server <- Rust client
    let port = pick_port();
    let mut child = Command::new(&server_exe)
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut sock = connect_retry(port);
    sock.write_all(b"hello sockets\n").unwrap();
    let mut reply = String::new();
    BufReader::new(sock.try_clone().unwrap())
        .read_line(&mut reply)
        .unwrap();
    assert_eq!(reply, "echo:hello sockets\n");
    let st = wait_exit(&mut child, Duration::from_secs(10));
    let mut stdout = String::new();
    child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    assert!(st.success(), "server exited {st:?}");
    assert!(stdout.contains("server:hello sockets"), "{stdout}");

    // Rust server <- Aoxn client
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port2 = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(s.try_clone().unwrap()).read_line(&mut line).unwrap();
        assert_eq!(line, "ping\n");
        s.write_all(b"pong\n").unwrap();
    });
    let out = run_timeout(Command::new(&client_exe).arg(port2.to_string()), Duration::from_secs(10));
    handle.join().unwrap();
    assert!(
        out.status.success(),
        "client: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "client:pong\n");

    // Connection refused is a checkable error, not a crash.
    let refused = pick_port();
    let out = run_timeout(Command::new(&client_exe).arg(refused.to_string()), Duration::from_secs(10));
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("ERR "),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );

    // A port already held by another listener fails bind with a value error.
    // Bind the SAME wildcard address the module uses; 127.0.0.1 and 0.0.0.0
    // can coexist on Windows and would make this test hang on accept.
    let held = TcpListener::bind("0.0.0.0:0").unwrap();
    let held_port = held.local_addr().unwrap().port();
    let out = run_timeout(
        Command::new(&server_exe).arg(held_port.to_string()),
        Duration::from_secs(10),
    );
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("ERR bind failed"), "{text}");

    drop(held);
    fs::remove_dir_all(&dir).unwrap();
}

fn build_with_selfhost(cli: &Path, dir: &Path, name: &str, src: &str) -> PathBuf {
    let path = dir.join(format!("{name}.ax"));
    fs::write(&path, src).unwrap();
    let exe = dir.join(format!("{name}{EXE}"));
    let mut cmd = Command::new(cli);
    cmd.current_dir(dir)
        .env("AOXN_STDLIB", root().join("stdlib"))
        .env("AOXN_CACHE_DIR", dir.join("cache"))
        .env("AOXN_CLANG", aoxn::find_clang().unwrap());
    cmd.args(["build", &format!("{name}.ax"), "-o", &format!("{name}{EXE}")]);
    if cfg!(windows) {
        cmd.args(["-l", "ws2_32"]);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "selfhost build {name}: {} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(exe.is_file());
    exe
}

#[test]
fn selfhost_built_socket_programs_roundtrip() {
    let Some(clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "clang required"
        );
        eprintln!("skipping: clang unavailable");
        return;
    };
    let dir = root().join("target").join(format!("socket-selfhost-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let cli = dir.join(format!("selfhost-cli{EXE}"));
    aoxn::build_paths_exe(
        &[root().join("selfhost/cli.ax").display().to_string()],
        &cli,
        true,
    )
    .expect("build the selfhost CLI");
    let server = build_with_selfhost(&cli, &dir, "self_server", SERVER);
    let client = build_with_selfhost(&cli, &dir, "self_client", CLIENT);

    let port = pick_port();
    let mut child = Command::new(&server)
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut sock = connect_retry(port);
    sock.write_all(b"from rust\n").unwrap();
    let mut reply = String::new();
    BufReader::new(sock.try_clone().unwrap())
        .read_line(&mut reply)
        .unwrap();
    assert_eq!(reply, "echo:from rust\n");
    let st = wait_exit(&mut child, Duration::from_secs(10));
    assert!(st.success(), "selfhost server exited {st:?}");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port2 = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(s.try_clone().unwrap()).read_line(&mut line).unwrap();
        assert_eq!(line, "ping\n");
        s.write_all(b"pong\n").unwrap();
    });
    let out = run_timeout(
        Command::new(&client).arg(port2.to_string()),
        Duration::from_secs(10),
    );
    handle.join().unwrap();
    assert!(out.status.success(), "selfhost client: {}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "client:pong\n");
    let _ = clang;
    fs::remove_dir_all(&dir).unwrap();
}
