import { chromium, expect } from '@playwright/test';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createSmokeServer } from './smoke-server.mjs';

// Attach only to a locally launched Flow webview, never to the user's browser.
// See README for the opt-in WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS launch command.
const endpoint = process.env.FLOW_CDP_URL ?? 'http://127.0.0.1:9223';
if (!['127.0.0.1', 'localhost'].includes(new URL(endpoint).hostname)) {
  throw new Error('The desktop smoke test requires a loopback CDP endpoint.');
}
const artifacts = resolve('.artifacts/desktop-smoke');
const destination = join(artifacts, 'downloads');
await mkdir(destination, { recursive: true });
const browser = await chromium.connectOverCDP(endpoint);
const pages = browser.contexts().flatMap(context => context.pages());
const candidates = [];
for (const page of pages) {
  if (await page.evaluate(() => Boolean(window.__TAURI_INTERNALS__))) candidates.push(page);
}
if (candidates.length !== 1) throw new Error(`Expected one Flow webview, found ${candidates.length}.`);
const page = candidates[0];
const failures = [];
page.on('pageerror', error => failures.push(error.message));
const invoke = (command, args) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const snapshot = () => invoke('get_snapshot');
const server = createSmokeServer();
await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
const base = `http://127.0.0.1:${server.address().port}`;
const prefix = `smoke-${Date.now()}`;
const initial = await snapshot();
if (initial.downloads.some(item => ['queued', 'downloading'].includes(item.status))) {
  throw new Error('Finish existing downloads before running the smoke test.');
}
const addButton = page.getByRole('button', { name: 'Add download', exact: true });
const dialog = page.getByRole('dialog', { name: 'Add a download' });
const log = message => console.log(`PASS ${message}`);

async function add(path, name) {
  await addButton.click();
  await dialog.getByLabel(/Download URL/).fill(`${base}${path}`);
  await dialog.getByLabel(/File name/).fill(name);
  await dialog.getByLabel('Save to', { exact: true }).fill(destination);
  await dialog.getByRole('button', { name: 'Start download' }).click();
  await expect(dialog).not.toBeVisible();
  const item = (await snapshot()).downloads.filter(item => item.url === `${base}${path}`).at(-1);
  if (!item) throw new Error('Added download was not returned by the real engine.');
  return item;
}

async function completed(item) {
  await expect.poll(async () => (await snapshot()).downloads.find(current => current.id === item.id)?.status, { timeout: 20_000 }).toBe('completed');
  await expect(page.getByRole('button', { name: `View details for ${item.fileName}`, exact: true })).toBeVisible();
  await page.getByRole('button', { name: `View details for ${item.fileName}`, exact: true }).click();
  await expect(page.getByRole('complementary', { name: 'Download details' }).getByText('Completed', { exact: true })).toBeVisible();
}

try {
  await expect(addButton).toBeEnabled({ timeout: 30_000 });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: join(artifacts, 'initial-light.png') });
  await addButton.click();
  await expect(dialog.getByLabel(/Download URL/)).toBeFocused();
  await dialog.getByRole('button', { name: 'Start download' }).click();
  await expect(dialog.getByText('Enter a download URL.')).toBeVisible();
  await dialog.getByLabel(/Download URL/).fill('ftp://example.com/file.txt');
  await dialog.getByRole('button', { name: 'Start download' }).click();
  await expect(dialog.getByText('Use a complete HTTP or HTTPS URL.')).toBeVisible();
  await dialog.getByLabel(/Download URL/).fill(`${base}/sample.txt`);
  await dialog.getByLabel(/File name/).fill('CON.txt');
  await dialog.getByRole('button', { name: 'Start download' }).click();
  await expect(dialog.getByText('This filename is reserved by Windows. Choose another name.')).toBeVisible();
  await page.screenshot({ path: join(artifacts, 'dialog-validation.png') });
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(addButton).toBeFocused();
  log('URL/filename validation and modal focus/Escape');

  const known = await add('/sample.txt', `${prefix}-sample.txt`);
  await expect.poll(async () => (await snapshot()).downloads.find(item => item.id === known.id)?.downloadedBytes ?? 0).toBeGreaterThan(0);
  await expect(page.getByRole('progressbar', { name: `Progress for ${known.fileName}` }).first()).toHaveAttribute('aria-valuenow');
  await page.screenshot({ path: join(artifacts, 'download-progress-light.png') });
  await completed(known);
  expect((await readFile(join(destination, known.fileName))).byteLength).toBe(131072);
  log('real known-size transfer, visible progress, completion and file contents');

  const details = page.getByRole('complementary', { name: 'Download details' });
  await details.getByRole('button', { name: 'Show in folder' }).click();
  await expect(details.getByRole('button', { name: 'Show in folder' })).toBeEnabled();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await details.getByRole('button', { name: 'Open file', exact: true }).click();
  await expect(details.getByRole('button', { name: 'Open file', exact: true })).toBeEnabled();
  await expect(page.getByRole('alert')).toHaveCount(0);
  log('real native open and reveal commands accepted');

  const saved = join(destination, known.fileName);
  await rename(saved, `${saved}.moved`);
  try {
    await details.getByRole('button', { name: 'Open file', exact: true }).click();
    await expect(page.getByRole('alert')).toContainText('moved or deleted');
  } finally { await rename(`${saved}.moved`, saved); }
  await page.getByRole('button', { name: 'Dismiss error' }).click();
  log('moved file returns a readable error');

  const unknown = await add('/stream.txt', `${prefix}-stream.txt`);
  const unknownProgress = page.getByRole('progressbar', { name: `Progress for ${unknown.fileName}` }).first();
  await expect(unknownProgress).not.toHaveAttribute('aria-valuenow');
  await expect(unknownProgress).toHaveClass(/indeterminate/);
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.screenshot({ path: join(artifacts, 'download-progress-dark.png') });
  await completed(unknown);
  await expect(unknownProgress).toHaveAttribute('aria-valuenow', '100');
  log('unknown-size transfer uses indeterminate progress then completed 100%');

  const duplicate = await add('/sample.txt', known.fileName);
  expect(duplicate.fileName).not.toBe(known.fileName);
  await completed(duplicate);
  expect(await readFile(join(destination, duplicate.fileName))).toEqual(await readFile(saved));
  log('duplicate filename preserves both files');

  const failed = await add('/missing.txt', `${prefix}-missing.txt`);
  await expect.poll(async () => (await snapshot()).downloads.find(item => item.id === failed.id)?.status).toBe('failed');
  await page.getByRole('navigation', { name: 'Download filters' }).getByRole('button', { name: /^Failed/ }).click();
  await page.getByRole('button', { name: `View details for ${failed.fileName}`, exact: true }).click();
  await expect(page.getByRole('note')).toContainText('404');
  log('HTTP failure appears in Failed filter and details');

  await page.getByRole('navigation', { name: 'Download filters' }).getByRole('button', { name: /^Completed/ }).click();
  await page.getByRole('searchbox').count().then(async count => {
    const search = count ? page.getByRole('searchbox') : page.getByRole('textbox', { name: 'Search downloads' });
    await search.fill(`${base}/stream.txt`);
    await expect(page.getByRole('button', { name: `View details for ${unknown.fileName}`, exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: `View details for ${known.fileName}`, exact: true })).toHaveCount(0);
    await search.fill('no-such-flow-download');
    await expect(page.getByText('No matching downloads')).toBeVisible();
    await search.fill('');
  });
  log('Completed filter, URL search and no-results state');

  const blocked = join(artifacts, `${prefix}-not-a-folder`);
  await writeFile(blocked, 'This is a file, not a destination directory.');
  await addButton.click();
  await dialog.getByLabel(/Download URL/).fill(`${base}/sample.txt`);
  await dialog.getByLabel('Save to', { exact: true }).fill(blocked);
  await dialog.getByRole('button', { name: 'Start download' }).click();
  await expect(dialog.getByRole('alert')).toContainText('not a directory');
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  log('invalid destination stays in the dialog with a readable error');

  await page.getByRole('navigation', { name: 'Download filters' }).getByRole('button', { name: /^All downloads/ }).click();
  await page.getByRole('button', { name: `View details for ${known.fileName}`, exact: true }).click();
  await page.screenshot({ path: join(artifacts, 'completed-dark.png') });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: join(artifacts, 'completed-light.png') });
  await page.setViewportSize({ width: 880, height: 620 });
  await page.screenshot({ path: join(artifacts, 'minimum-window.png') });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.setViewportSize({ width: 1280, height: 780 });
  await writeFile(join(artifacts, 'result.json'), JSON.stringify({ ids: [known.id, unknown.id, duplicate.id, failed.id], prefix, destination, failures }, null, 2));
  expect(failures).toEqual([]);
  log('light/dark screenshots, minimum width, no uncaught frontend errors');
  console.log(`Screenshots and download fixtures: ${artifacts}`);
} finally {
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  await browser.close();
}
