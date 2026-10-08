const [command, args, done] = arguments
window.__TAURI__.core.invoke(command, args).then(
  value => done({ ok: value ?? null }),
  error => done({ err: String(error) }),
)
