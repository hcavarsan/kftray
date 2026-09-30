export async function runWithLimit<T>(
  items: T[],
  limit: number,
  worker: (item: T) => Promise<void>,
) {
  let nextIndex = 0

  const runWorker = async () => {
    while (nextIndex < items.length) {
      const item = items[nextIndex]

      nextIndex += 1
      await worker(item)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, runWorker),
  )
}
