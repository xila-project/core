// Fill the existing disk cache, retaining only the current network chunk.
// Persistence is the caller's responsibility after mounting and initializing it.
export async function importSeed(url, data) {
  const response = await fetch(url);
  if (!response.ok || !response.body) {
    throw new Error(`Cannot fetch reference disk: HTTP ${response.status}`);
  }
  const reader = response.body.getReader();
  let offset = 0;
  let complete = false;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value.byteLength > data.byteLength - offset) {
        throw new Error("Reference disk exceeds configured capacity");
      }
      data.set(value, offset);
      offset += value.byteLength;
    }
    if (offset !== data.byteLength) {
      throw new Error("Reference disk has the wrong size");
    }
    complete = true;
  } finally {
    if (!complete) await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
