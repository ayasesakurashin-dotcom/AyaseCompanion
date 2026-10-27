// Inspect every reachable public blob; report rule names, never matched secrets.
import {spawnSync} from 'node:child_process';
import {readFileSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
function git(args, binary = false) {
  const result = spawnSync('git', args, {cwd: root, maxBuffer: 128 * 1024 * 1024});
  if (result.status !== 0) throw new Error('Git audit input could not be read');
  return binary ? result.stdout : result.stdout.toString('utf8').trimEnd();
}
const args = process.argv.slice(2);
let ref = 'HEAD';
let index = false;
const binaries = [];
while (args.length) {
  const arg = args.shift();
  if (arg === '--ref') ref = args.shift();
  else if (arg === '--index') index = true;
  else if (arg === '--binary') binaries.push(args.shift());
  else throw new Error('Unknown audit option');
}
if (!ref || binaries.some(value => !value) || ref.startsWith('refs/private/')) throw new Error('Invalid public audit target');
const findings = new Set();
const excluded = /^(?:artifacts|local-assets|docs\/validation)(?:\/|$)|^docs\/(?:P0-P1.*|P\d.*(?:实机|专项).*)\.md$|(?:^|\/)(?:\.env(?!\.example$)|ayase-diagnostics-|codex-clipboard-)|\.(?:bundle|pdb|dmp|db|sqlite3)(?:$|-)|\.local\.json$/i;
const secrets = [
  /\bgh[pousr]_[A-Za-z0-9]{30,}\b/,
  /\bgithub_pat_[A-Za-z0-9_]{40,}\b/,
  /\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{40,}\b/,
  /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/,
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/,
];
const machinePrefixes = [root, path.dirname(root), process.env.USERPROFILE, process.env.CARGO_HOME, process.env.RUSTUP_HOME]
  .filter(Boolean).flatMap(value => [value, value.replaceAll('\\', '/')]).map(value => value.toLowerCase());
function inspect(name, bytes, binary = false) {
  if (!binary && excluded.test(name)) findings.add(`${name}: private file`);
  const texts = [bytes.toString('utf8')];
  if (binary) texts.push(bytes.toString('utf16le'), bytes.subarray(1).toString('utf16le'));
  for (const text of texts) {
    const lower = text.toLowerCase();
    if (machinePrefixes.some(prefix => lower.includes(prefix))) findings.add(`${name}: build-machine path`);
    if (secrets.some(pattern => pattern.test(text))) findings.add(`${name}: credential pattern`);
    // The package security test deliberately uses one synthetic absolute path.
    const pathText = name === 'crates/ayase-core/src/package.rs' ? text.replaceAll('C:/x', 'synthetic-path') : text;
    if (/(?<![A-Za-z0-9])[A-Za-z]:[\\/](?!\/)[A-Za-z0-9_. -]+[\\/][A-Za-z0-9_]/.test(pathText)) findings.add(`${name}: absolute machine path`);
    if (/\/Users\/[A-Za-z0-9_.-]+\/|\/home\/[A-Za-z0-9_.-]+\//.test(text)) findings.add(`${name}: home-directory path`);
  }
}
let objects;
if (index) {
  objects = git(['ls-files', '--stage']).split('\n').filter(Boolean).map(line => {
    const match = /^\d+ ([a-f0-9]+) \d\t(.+)$/.exec(line);
    if (!match) throw new Error('Unrecognized index entry');
    return {oid: match[1], name: match[2]};
  });
} else {
  git(['rev-parse', '--verify', `${ref}^{commit}`]);
  const names = git(['-c', 'core.quotepath=false', 'log', '--format=', '--name-only', ref]).split('\n');
  for (const name of names) if (excluded.test(name)) findings.add(`${name}: private history file`);
  inspect('commit metadata', Buffer.from(git(['log', '--format=fuller', ref])));
  objects = git(['rev-list', '--objects', ref]).split('\n').filter(Boolean).map(line => {
    const space = line.indexOf(' ');
    return {oid: space < 0 ? line : line.slice(0, space), name: space < 0 ? 'Git object' : line.slice(space + 1)};
  });
}
const batch = spawnSync('git', ['cat-file', '--batch'], {
  cwd: root, input: objects.map(object => object.oid).join('\n') + '\n', maxBuffer: 128 * 1024 * 1024,
});
if (batch.status !== 0) throw new Error('Git objects could not be read');
let offset = 0;
let blobCount = 0;
for (const object of objects) {
  const end = batch.stdout.indexOf(10, offset);
  const header = batch.stdout.subarray(offset, end).toString('ascii').split(' ');
  const size = Number(header[2]);
  if (!Number.isSafeInteger(size)) throw new Error('Invalid Git object size');
  offset = end + 1;
  if (header[1] === 'blob') { inspect(object.name, batch.stdout.subarray(offset, offset + size)); blobCount++; }
  offset += size + 1;
}
for (const file of binaries) inspect(path.basename(file), readFileSync(file), true);
if (findings.size) {
  for (const finding of findings) process.stderr.write(finding + '\n');
  process.exitCode = 1;
} else {
  process.stdout.write(`Public audit passed: ${blobCount} source blobs, ${binaries.length} binaries.\n`);
}
