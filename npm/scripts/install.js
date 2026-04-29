const fs = require('fs');
const path = require('path');
const https = require('https');
const crypto = require('crypto');

const VERSION = require('../package.json').version;
const REPO = 'suisya-systems/renga';
const MAX_REDIRECTS = 5;
const ALLOWED_REDIRECT_HOSTS = new Set([
  'github.com',
  'objects.githubusercontent.com',
  'release-assets.githubusercontent.com',
  'github-releases.githubusercontent.com',
]);

function getPlatformBinary() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === 'win32' && arch === 'x64') return 'renga-windows-x64.exe';
  if (platform === 'darwin' && arch === 'arm64') return 'renga-macos-arm64';
  if (platform === 'darwin' && arch === 'x64') return 'renga-macos-x64';
  if (platform === 'linux' && arch === 'x64') return 'renga-linux-x64';

  console.error(`Unsupported platform: ${platform}-${arch}`);
  process.exit(1);
}

function resolveAndValidateUrl(rawUrl, baseUrl) {
  const parsed = new URL(rawUrl, baseUrl);
  if (parsed.protocol !== 'https:') {
    throw new Error(`Refusing non-HTTPS URL: ${parsed.toString()}`);
  }
  if (!ALLOWED_REDIRECT_HOSTS.has(parsed.hostname)) {
    throw new Error(`Refusing download from unexpected host: ${parsed.hostname}`);
  }
  return parsed;
}

function download(url, dest, redirects = 0, baseUrl) {
  return new Promise((resolve, reject) => {
    if (redirects > MAX_REDIRECTS) {
      reject(new Error(`Too many redirects (max ${MAX_REDIRECTS})`));
      return;
    }
    let requestUrl;
    try {
      requestUrl = resolveAndValidateUrl(url, baseUrl);
    } catch (err) {
      reject(err);
      return;
    }
    https.get(requestUrl, { headers: { 'User-Agent': 'renga-installer' } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        download(res.headers.location, dest, redirects + 1, requestUrl).then(resolve, reject);
        return;
      }
      if (res.statusCode !== 200) {
        reject(new Error(`Download failed: HTTP ${res.statusCode}`));
        return;
      }
      const file = fs.createWriteStream(dest);
      file.on('error', reject);
      res.pipe(file);
      file.on('finish', () => {
        file.close();
        resolve();
      });
    }).on('error', reject);
  });
}

function fetchText(url, redirects = 0, baseUrl) {
  return new Promise((resolve, reject) => {
    if (redirects > MAX_REDIRECTS) {
      reject(new Error(`Too many redirects (max ${MAX_REDIRECTS})`));
      return;
    }
    let requestUrl;
    try {
      requestUrl = resolveAndValidateUrl(url, baseUrl);
    } catch (err) {
      reject(err);
      return;
    }
    https.get(requestUrl, { headers: { 'User-Agent': 'renga-installer' } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        fetchText(res.headers.location, redirects + 1, requestUrl).then(resolve, reject);
        return;
      }
      if (res.statusCode !== 200) {
        reject(new Error(`Fetch failed: HTTP ${res.statusCode}`));
        return;
      }
      let data = '';
      res.on('data', (chunk) => { data += chunk; });
      res.on('end', () => resolve(data));
    }).on('error', reject);
  });
}

function sha256(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash('sha256');
    const stream = fs.createReadStream(filePath);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.on('end', () => resolve(hash.digest('hex')));
    stream.on('error', reject);
  });
}

function getExpectedChecksum(checksums, binaryName) {
  for (const line of checksums.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const match = trimmed.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (!match) continue;
    if (match[2] === binaryName) {
      return match[1].toLowerCase();
    }
  }
  throw new Error(`Checksum entry not found for ${binaryName}`);
}

function cleanupFile(filePath) {
  try {
    fs.rmSync(filePath, { force: true });
  } catch (_) {
    // Best-effort cleanup only.
  }
}

// Replace `dest` with `tempDest`.
//
// Not truly atomic: on Windows a POSIX-style rename isn't available,
// and even on Unix we run two renames back-to-back (dest -> backup,
// then tempDest -> dest), so there is a brief window where `dest`
// does not exist. If the process is killed inside that window the
// user is left with no binary at the canonical path until they
// rerun the installer.
//
// The sequence is still a strong improvement over the previous
// "rm old, rename new" path because:
//
// - The old binary is preserved at a unique backup path for as long
//   as the install could fail. A rename-level failure restores it.
// - The backup path is per-install (suffix with process PID and a
//   timestamp) so a stale `.bak` from a crashed previous run isn't
//   clobbered, and a failed install leaves behind a file the user
//   can inspect or restore manually.
// - The rename can fail on Windows when an antivirus scanner or
//   another process briefly holds the old binary open (EBUSY /
//   EPERM / EACCES). A small bounded retry with exponential
//   backoff (50 / 100 / 200 / 400 ms between attempts, five
//   attempts total, ~750 ms total wait before giving up) absorbs
//   most of that transient lock contention.
function replaceBinaryWithBackup(tempDest, dest) {
  const backup = `${dest}.${process.pid}.${Date.now()}.bak`;
  const hadExisting = fs.existsSync(dest);

  if (hadExisting) {
    fs.renameSync(dest, backup);
  }

  const maxAttempts = 5;
  let lastErr = null;
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      fs.renameSync(tempDest, dest);
      lastErr = null;
      break;
    } catch (err) {
      lastErr = err;
      const retryable =
        err && (err.code === 'EBUSY' || err.code === 'EPERM' || err.code === 'EACCES');
      if (!retryable || attempt === maxAttempts) break;
      const waitMs = 50 * Math.pow(2, attempt - 1);
      const until = Date.now() + waitMs;
      while (Date.now() < until) {
        // Busy-wait is fine here; install.js is short-lived and
        // single-threaded, and we want to stay synchronous.
      }
    }
  }

  if (lastErr) {
    if (hadExisting) {
      try {
        fs.renameSync(backup, dest);
      } catch (restoreErr) {
        lastErr = new Error(
          `Failed to install new binary and failed to restore old binary.\n` +
            `  Install error: ${lastErr.message}\n` +
            `  Restore error: ${restoreErr.message}\n` +
            `  Backup lives at: ${backup}`
        );
      }
    }
    throw lastErr;
  }

  if (hadExisting) {
    cleanupFile(backup);
  }
}

async function main() {
  const binaryName = getPlatformBinary();
  const baseUrl = `https://github.com/${REPO}/releases/download/v${VERSION}`;
  const url = `${baseUrl}/${binaryName}`;
  const binDir = path.join(__dirname, '..', 'bin');
  const isWindows = process.platform === 'win32';
  const dest = path.join(binDir, isWindows ? 'renga.exe' : 'renga');
  const tempDest = `${dest}.tmp`;

  console.log(`Downloading renga v${VERSION} for ${process.platform}-${process.arch}...`);

  try {
    fs.mkdirSync(binDir, { recursive: true });
    cleanupFile(tempDest);

    await download(url, tempDest);

    const checksums = await fetchText(`${baseUrl}/checksums.txt`);
    const expectedHash = getExpectedChecksum(checksums, binaryName);
    const actualHash = await sha256(tempDest);
    if (actualHash !== expectedHash) {
      throw new Error(
        [
          'Checksum verification FAILED - downloaded binary does not match.',
          `  Expected: ${expectedHash}`,
          `  Actual:   ${actualHash}`,
        ].join('\n')
      );
    }
    console.log('Checksum verified.');

    if (!isWindows) {
      fs.chmodSync(tempDest, 0o755);
    }

    replaceBinaryWithBackup(tempDest, dest);

    const BLUE = '\x1b[38;2;88;166;255m';
    const DIM = '\x1b[38;2;110;118;129m';
    const RESET = '\x1b[0m';
    console.log('');
    console.log(`${BLUE}██████╗ ███████╗███╗   ██╗ ██████╗  █████╗ ${RESET}`);
    console.log(`${BLUE}██╔══██╗██╔════╝████╗  ██║██╔════╝ ██╔══██╗${RESET}`);
    console.log(`${BLUE}██████╔╝█████╗  ██╔██╗ ██║██║  ███╗███████║${RESET}`);
    console.log(`${BLUE}██╔══██╗██╔══╝  ██║╚██╗██║██║   ██║██╔══██║${RESET}`);
    console.log(`${BLUE}██║  ██║███████╗██║ ╚████║╚██████╔╝██║  ██║${RESET}`);
    console.log(`${BLUE}╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝ ╚═════╝ ╚═╝  ╚═╝${RESET}`);
    console.log('');
    console.log(`${DIM}  AI-native terminal for agent teams v${VERSION}${RESET}`);
    console.log(`${DIM}  Run 'renga' to start.${RESET}`);
    console.log('');
  } catch (err) {
    cleanupFile(tempDest);
    // `err.message` already carries the specific failing resource
    // when it comes from resolveAndValidateUrl (host / scheme),
    // getExpectedChecksum (checksum entry lookup), the Checksum
    // verification FAILED branch, or replaceBinaryAtomically
    // (install + restore detail). Print it verbatim, and add the
    // binary URL as the default download context for the manual
    // fallback path.
    console.error(`Failed to install renga: ${err.message}`);
    console.error(`Binary URL: ${url}`);
    console.error(`Checksums URL: ${baseUrl}/checksums.txt`);
    console.error('You can download manually from: https://github.com/suisya-systems/renga/releases');
    process.exit(1);
  }
}

main();
