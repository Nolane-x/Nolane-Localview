import { execFileSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  renameSync,
} from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const desktopDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(desktopDir, '..', '..');
const release = process.argv.includes('--release');
const profile = release ? 'release' : 'debug';
const extension = process.platform === 'win32' ? '.exe' : '';
const targetTriple = execFileSync('rustc', ['--print', 'host-tuple'], {
  cwd: repoRoot,
  encoding: 'utf8',
}).trim();

if (!targetTriple) {
  throw new Error('rustc did not report a host target triple');
}

const cargoArgs = ['build', '-p', 'localview-daemon'];
if (release) cargoArgs.push('--release');
execFileSync('cargo', cargoArgs, { cwd: repoRoot, stdio: 'inherit' });

const source = path.join(repoRoot, 'target', profile, `localview-daemon${extension}`);
if (!existsSync(source)) {
  throw new Error(`LocalView daemon build output is missing: ${source}`);
}

const binaryDir = path.join(desktopDir, 'src-tauri', 'binaries');
mkdirSync(binaryDir, { recursive: true });
const destination = path.join(
  binaryDir,
  `localview-daemon-${targetTriple}${extension}`,
);
const staged = `${destination}.tmp-${process.pid}`;
copyFileSync(source, staged);
if (process.platform !== 'win32') chmodSync(staged, 0o755);
renameSync(staged, destination);

console.log(`Prepared LocalView daemon sidecar: ${destination}`);
