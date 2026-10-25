import assert from 'node:assert/strict';
import test from 'node:test';

import { diskNameForDate, isManagedDiskName, nextDiskName } from '../disk_names.mjs';

test('new disk names use the local calendar date and suffix collisions', () => {
  const date = new Date(2026, 9, 16, 23, 30);
  assert.equal(diskNameForDate(date, 1), 'xila_2026-10-16.img');
  assert.equal(diskNameForDate(date, 2), 'xila_2026-10-16-2.img');
  assert.equal(
    nextDiskName(date, ['xila_2026-10-16.img', 'xila_2026-10-16-2.img']),
    'xila_2026-10-16-3.img',
  );
});

test('only Xila disk images are managed by the panel', () => {
  assert.equal(isManagedDiskName('xila_drive.img'), true);
  assert.equal(isManagedDiskName('xila_2026-10-16-2.img'), true);
  assert.equal(isManagedDiskName('unrelated.img'), false);
  assert.equal(isManagedDiskName('xila_2026-10-16.txt'), false);
});
