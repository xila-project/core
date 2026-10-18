import { test } from "node:test";
import assert from "node:assert/strict";
import { importSeed } from "../src/devices/seed.mjs";

async function withResponse(response, action) {
  const original = globalThis.fetch;
  globalThis.fetch = async () => response;
  try {
    await action();
  } finally {
    globalThis.fetch = original;
  }
}

test("streams exact bytes into the provided cache", async () => {
  const bytes = new Uint8Array(512);
  bytes.fill(42, 0, 250);
  bytes.fill(17, 250);
  const stream = new ReadableStream({
    start(c) {
      c.enqueue(bytes.subarray(0, 250));
      c.enqueue(bytes.subarray(250));
      c.close();
    },
  });
  const cache = new Uint8Array(512);
  await withResponse(new Response(stream), () => importSeed("seed.img", cache));
  assert.deepEqual(cache, bytes);
});

test("rejects short and oversized bodies and HTTP failures", async () => {
  for (const response of [
    new Response(new Uint8Array(511)),
    new Response(new Uint8Array(513)),
    new Response("missing", { status: 404 }),
  ]) {
    await withResponse(response, async () =>
      assert.rejects(importSeed("seed.img", new Uint8Array(512))),
    );
  }
});

test("propagates network failures and cancels an overflowing reader", async () => {
  let cancelled = false;
  const stream = new ReadableStream({
    start(c) {
      c.enqueue(new Uint8Array(513));
    },
    cancel() {
      cancelled = true;
    },
  });
  await withResponse(new Response(stream), async () =>
    assert.rejects(importSeed("seed.img", new Uint8Array(512))),
  );
  assert.equal(cancelled, true);
  const original = globalThis.fetch;
  globalThis.fetch = async () => {
    throw new Error("offline");
  };
  try {
    await assert.rejects(
      importSeed("seed.img", new Uint8Array(512)),
      /offline/,
    );
  } finally {
    globalThis.fetch = original;
  }
});
