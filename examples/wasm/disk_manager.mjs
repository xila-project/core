import { isManagedDiskName, nextDiskName } from './disk_names.mjs';

const LEGACY_DISK = 'xila_drive.img';
const SELECTED_DISK_KEY = 'xila.selectedDisk';
const DISK_LIST_LOCK = 'xila-opfs-disk-list';

const byId = (id) => document.getElementById(id);
const list = byId('disk-list');
const status = byId('disk-status');
const selectButton = byId('disk-select');
const createButton = byId('disk-create');
const removeButton = byId('disk-remove');
let activeDisk = null;
let runtimeReady = false;

function setStatus(message, isError = false) {
  status.textContent = message;
  status.dataset.error = String(isError);
}

function selectedDisk() {
  return localStorage.getItem(SELECTED_DISK_KEY) || LEGACY_DISK;
}

async function diskFiles() {
  const root = await navigator.storage.getDirectory();
  const files = [];
  for await (const [name, handle] of root.entries()) {
    if (handle.kind === 'file' && isManagedDiskName(name)) files.push(name);
  }
  return files.sort((left, right) => left.localeCompare(right));
}

function updateControls() {
  selectButton.disabled = !runtimeReady || list.options.length === 0;
  createButton.disabled = !runtimeReady;
  removeButton.disabled = !runtimeReady || !list.value || list.value === activeDisk;
}

function render(files) {
  const selected = selectedDisk();
  list.replaceChildren();
  for (const name of files) {
    const option = document.createElement('option');
    option.value = name;
    option.textContent = name === activeDisk ? `${name} (open)` : name;
    option.selected = name === selected;
    list.append(option);
  }
  updateControls();
}

async function refresh(message = 'Choose a disk, or create a new one.') {
  try {
    render(await diskFiles());
    setStatus(message);
  } catch (error) {
    setStatus(`Browser storage is unavailable: ${error.message}`, true);
    selectButton.disabled = createButton.disabled = removeButton.disabled = true;
  }
}

function requestDiskSwitch(name) {
  localStorage.setItem(SELECTED_DISK_KEY, name);
  setStatus(`Saving and switching to ${name}…`);
  selectButton.disabled = createButton.disabled = removeButton.disabled = true;
  window.dispatchEvent(new Event('xila-disk-switch'));
}

list.addEventListener('change', updateControls);
selectButton.addEventListener('click', () => {
  const name = list.value;
  if (!name || name === activeDisk) return;
  if (window.confirm(`Save and restart Xila using ${name}?`)) requestDiskSwitch(name);
});

createButton.addEventListener('click', async () => {
  createButton.disabled = true;
  try {
    const root = await navigator.storage.getDirectory();
    const name = await navigator.locks.request(DISK_LIST_LOCK, async () => {
      const files = new Set(await diskFiles());
      const candidate = nextDiskName(new Date(), files);
      await root.getFileHandle(candidate, { create: true });
      return candidate;
    });
    await refresh(`${name} created. Select it to restart Xila on that disk.`);
    list.value = name;
    updateControls();
    if (window.confirm(`Save and restart Xila using ${name}?`)) requestDiskSwitch(name);
  } catch (error) {
    setStatus(`Could not create a disk: ${error.message}`, true);
    createButton.disabled = false;
  }
});

removeButton.addEventListener('click', async () => {
  const name = list.value;
  if (!name || name === activeDisk) return;
  if (!window.confirm(`Permanently remove ${name} and all files stored on it?`)) return;
  removeButton.disabled = true;
  try {
    const removed = await navigator.locks.request(`xila-opfs:${name}`, { ifAvailable: true }, async (lock) => {
      if (!lock) return false;
      await (await navigator.storage.getDirectory()).removeEntry(name);
      return true;
    });
    if (!removed) throw new Error('This disk is open in another Xila tab. Close that tab first.');
    if (selectedDisk() === name) localStorage.setItem(SELECTED_DISK_KEY, LEGACY_DISK);
    await refresh(`${name} removed.`);
  } catch (error) {
    setStatus(`Could not remove ${name}: ${error.message}`, true);
    render(await diskFiles().catch(() => []));
  }
});

window.xilaDiskManagerReady = (name) => {
  activeDisk = name;
  runtimeReady = true;
  renderFromStorage();
};

window.xilaDiskManagerError = (message) => {
  if (activeDisk) localStorage.setItem(SELECTED_DISK_KEY, activeDisk);
  setStatus(message, true);
  runtimeReady = true;
  renderFromStorage();
};

async function renderFromStorage() {
  render(await diskFiles().catch(() => []));
}

refresh();
