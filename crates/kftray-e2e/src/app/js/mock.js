const [command, value] = arguments
const mocks = (window.__e2eMocks ??= new Map())
mocks.set(command, value)
window.__e2eFetch ??= window.fetch
window.fetch = (input, init) => {
  const url = new URL(typeof input === 'string' ? input : input.url)
  const ipc = url.protocol === 'ipc:' || url.hostname === 'ipc.localhost'
  const cmd = decodeURIComponent(url.pathname.slice(1))
  return ipc && mocks.has(cmd)
    ? Promise.resolve(
        new Response(JSON.stringify(mocks.get(cmd)), {
          headers: { 'Tauri-Response': 'ok', 'Content-Type': 'application/json' },
        }),
      )
    : window.__e2eFetch(input, init)
}
