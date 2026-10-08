const [command, value] = arguments
const internals = window.__TAURI_INTERNALS__
internals.__e2eOriginal ??= internals.invoke
const mocks = (internals.__e2eMocks ??= new Map())
mocks.set(command, value)
internals.invoke = (cmd, args, options) =>
  mocks.has(cmd) ? Promise.resolve(mocks.get(cmd)) : internals.__e2eOriginal(cmd, args, options)
