use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{
    AsyncBufReadExt,
    AsyncReadExt,
    AsyncWriteExt,
    BufReader,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::process::Command;

fn kube_response(status: &str, body: serde_json::Value) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn serve_fake_kube_api() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let line = request.lines().next().unwrap_or_default();
                let response = if line.contains("/version") {
                    kube_response(
                        "200 OK",
                        serde_json::json!({
                            "major": "1", "minor": "30", "gitVersion": "v1.30.0",
                            "gitCommit": "x", "gitTreeState": "clean",
                            "buildDate": "2024-01-01T00:00:00Z", "goVersion": "go1",
                            "compiler": "gc", "platform": "linux/amd64"
                        }),
                    )
                } else if line.contains("/api/v1/namespaces") {
                    kube_response(
                        "200 OK",
                        serde_json::json!({
                            "apiVersion": "v1", "kind": "NamespaceList",
                            "metadata": {"resourceVersion": "1"},
                            "items": [{"metadata": {"name": "team-a"}}]
                        }),
                    )
                } else {
                    kube_response(
                        "404 Not Found",
                        serde_json::json!({
                            "kind": "Status", "apiVersion": "v1", "metadata": {},
                            "status": "Failure", "reason": "NotFound", "code": 404,
                            "message": "not found"
                        }),
                    )
                };
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    addr
}

#[tokio::test]
async fn standalone_server_answers_a_cluster_tool_call() {
    let api = serve_fake_kube_api().await;
    let dir = tempfile::tempdir().unwrap();
    let kubeconfig = dir.path().join("kubeconfig");
    std::fs::write(
        &kubeconfig,
        format!(
            "apiVersion: v1\nkind: Config\ncurrent-context: dev\nclusters:\n- name: dev\n  cluster:\n    server: http://{api}\ncontexts:\n- name: dev\n  context:\n    cluster: dev\n    user: dev\nusers:\n- name: dev\n  user:\n    token: test\n"
        ),
    )
    .unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir(&config_dir).unwrap();

    let mut server = Command::new(env!("CARGO_BIN_EXE_kftray-mcp"))
        .args(["--port", "0"])
        .env("KFTRAY_CONFIG", &config_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let mut log = BufReader::new(server.stderr.take().unwrap()).lines();
    let addr = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = log.next_line().await.unwrap() {
            if let Some((_, addr)) = line.split_once("MCP server listening on http://") {
                return addr.trim().to_string();
            }
        }
        panic!("kftray-mcp exited before it started listening");
    })
    .await
    .expect("kftray-mcp did not start listening within 30s");
    tokio::spawn(async move { while let Ok(Some(_)) = log.next_line().await {} });

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "list_namespaces",
            "arguments": {"context": "dev", "kubeconfig": kubeconfig}
        }
    })
    .to_string();
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    stream
        .write_all(
            format!(
                "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut reply = String::new();
    tokio::time::timeout(Duration::from_secs(30), stream.read_to_string(&mut reply))
        .await
        .expect("no reply within 30s")
        .unwrap();

    assert!(reply.starts_with("HTTP/1.1 200"), "reply: {reply:?}");
    assert!(reply.contains("team-a"), "reply: {reply:?}");
}
