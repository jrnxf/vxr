// What the Worker accepts as a stored file: a MagicaVoxel .vox (the "VOX " magic, a version, one
// MAIN chunk whose children fill it exactly, every chunk's sizes inside its parent) and, for
// thumbnails, a small PNG. A cheap structural check, not a decode: the editor reads the file.

const text = new TextDecoder('latin1');

function id(bytes: Uint8Array, at: number): string {
  return text.decode(bytes.subarray(at, at + 4));
}

/** Why `bytes` is not a .vox file, or null when it is one. */
export function voxProblem(bytes: Uint8Array): string | null {
  if (bytes.length < 20) return 'too short';
  if (id(bytes, 0) !== 'VOX ') return 'not a .vox file';
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (id(bytes, 8) !== 'MAIN') return 'no MAIN chunk';
  const mainContent = view.getUint32(12, true);
  const mainChildren = view.getUint32(16, true);
  if (20 + mainContent + mainChildren !== bytes.length) return 'MAIN does not fill the file';
  // Walk MAIN's children (and theirs), each chunk inside the one around it.
  const stack: number[] = [bytes.length];
  let at = 20 + mainContent;
  let chunks = 0;
  while (stack.length) {
    const end = stack[stack.length - 1];
    if (at === end) {
      stack.pop();
      continue;
    }
    if (at + 12 > end) return 'a chunk runs past its parent';
    if (!/^[\x20-\x7e]{4}$/.test(id(bytes, at))) return 'a chunk has no readable id';
    const content = view.getUint32(at + 4, true);
    const children = view.getUint32(at + 8, true);
    const childStart = at + 12 + content;
    const next = childStart + children;
    if (next > end) return 'a chunk runs past its parent';
    if (++chunks > 1_000_000) return 'too many chunks';
    at = childStart;
    if (children) stack.push(next);
    else at = next;
  }
  return null;
}

const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
/** The largest thumbnail the Worker keeps: the editor's are 160 px PNGs of a few KB. */
export const THUMB_BYTES = 256 * 1024;
export const THUMB_SIDE = 512;

/** Why `bytes` is not a thumbnail the Worker keeps, or null when it is one. */
export function thumbProblem(bytes: Uint8Array): string | null {
  if (bytes.length > THUMB_BYTES) return 'thumbnail too large';
  if (bytes.length < 33 || PNG.some((b, i) => bytes[i] !== b)) return 'not a PNG';
  if (id(bytes, 12) !== 'IHDR') return 'not a PNG';
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const w = view.getUint32(16);
  const h = view.getUint32(20);
  if (!w || !h || w > THUMB_SIDE || h > THUMB_SIDE) return 'thumbnail too large';
  return null;
}
