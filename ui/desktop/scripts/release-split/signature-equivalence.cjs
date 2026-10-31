const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');

const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function normalizedLinkedit(bytes) {
  if (bytes.length < 32 || bytes.readUInt32LE(0) !== 0xfeedfacf ||
      ![0x0100000c, 0x01000007].includes(bytes.readUInt32LE(4)))
    throw new Error('Unsupported Mach-O architecture: expected thin arm64 or x86_64');
  const commandsEnd = 32 + bytes.readUInt32LE(20);
  if (commandsEnd > bytes.length) throw new Error('Invalid load-command bounds');
  let cursor = 32;
  let linkedit;
  for (let index = 0; index < bytes.readUInt32LE(16); index++) {
    if (cursor + 8 > commandsEnd) throw new Error('Truncated load command');
    const command = bytes.readUInt32LE(cursor);
    const size = bytes.readUInt32LE(cursor + 4);
    if (size < 8 || cursor + size > commandsEnd) throw new Error('Invalid load-command size');
    if (command === 0x1d) throw new Error('Signature must be removed from comparison copies first');
    if (command === 0x19 && size >= 72 && bytes.subarray(cursor + 8, cursor + 24).toString().replace(/\0+$/, '') === '__LINKEDIT') {
      if (linkedit) throw new Error('Multiple __LINKEDIT segments');
      const fileOffset = bytes.readBigUInt64LE(cursor + 40);
      const fileSize = bytes.readBigUInt64LE(cursor + 48);
      const vmSize = bytes.readBigUInt64LE(cursor + 32);
      if (bytes.readInt32LE(cursor + 56) !== 1 || bytes.readInt32LE(cursor + 60) !== 1 || bytes.readUInt32LE(cursor + 64) !== 0)
        throw new Error('__LINKEDIT must be read-only and have no sections');
      if (fileOffset < BigInt(commandsEnd) || fileOffset + fileSize !== BigInt(bytes.length) || vmSize < fileSize)
        throw new Error('Invalid stripped __LINKEDIT bounds');
      linkedit = { vmSize: Number(vmSize), fieldOffset: cursor + 32, fileOffset: Number(fileOffset), fileSize: Number(fileSize) };
    }
    cursor += size;
  }
  if (!linkedit || cursor !== commandsEnd) throw new Error('Missing segment or invalid command coverage');
  const normalized = Buffer.from(bytes);
  normalized.writeBigUInt64LE(BigInt(linkedit.fileSize), linkedit.fieldOffset);
  return { normalized, linkedit };
}

function compareStripped(original, signed) {
  const a = normalizedLinkedit(original);
  const b = normalizedLinkedit(signed);
  if (!a.normalized.equals(b.normalized)) throw new Error('CLI differs beyond the read-only __LINKEDIT allocation field');
  return { original: a.linkedit, signed: b.linkedit, equivalentSha256: sha256(a.normalized), byteLength: original.length };
}

function compareSignedCli(originalPath, signedPath) {
  const temporary = fs.mkdtempSync('/tmp/goose-cli-equivalence-');
  try {
    const original = path.join(temporary, 'original');
    const signed = path.join(temporary, 'signed');
    fs.copyFileSync(originalPath, original);
    fs.copyFileSync(signedPath, signed);
    execFileSync('codesign', ['--remove-signature', original]);
    execFileSync('codesign', ['--remove-signature', signed]);
    return compareStripped(fs.readFileSync(original), fs.readFileSync(signed));
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

module.exports = { compareSignedCli, compareStripped };
if (require.main === module) {
  console.log(JSON.stringify(compareSignedCli(process.argv[2], process.argv[3]), null, 2));
}
