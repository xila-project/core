const LEGACY_DISK = 'xila_drive.img';

export function diskNameForDate(date, index = 1) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const suffix = index === 1 ? '' : `-${index}`;
  return `xila_${year}-${month}-${day}${suffix}.img`;
}

export function nextDiskName(date, existingNames) {
  const existing = new Set(existingNames);
  let index = 1;
  let name = diskNameForDate(date, index);
  while (existing.has(name)) name = diskNameForDate(date, ++index);
  return name;
}

export function isManagedDiskName(name) {
  return name === LEGACY_DISK || /^xila_\d{4}-\d{2}-\d{2}(?:-\d+)?\.img$/.test(name);
}
