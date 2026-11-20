// Reading a connection failure for what would actually fix it (#331).
//
// Most failures are a tunnel or socket that died, and reconnecting is the
// right offer. A private key that cannot be loaded is different: retrying
// reads the same file and fails the same way every time, so the person needs
// the edit form instead. The tunnel's message is the contract here
// (`TunnelError::KeyLoad` in dbboard-tunnel): it names the path in quotes,
// then the reason after `': `.

export type KeyLoadReason = 'missing' | 'ppk' | 'unreadable';

export interface KeyLoadFailure {
  path: string;
  reason: KeyLoadReason;
}

const KEY_LOAD = /failed to load ssh private key '(.*)': (.*)$/s;
// os error 2 / 3: file or directory not found, on Unix and on Windows.
const NOT_FOUND = /\(os error [23]\)|no such file or directory|cannot find the (file|path)/i;

/** The key-load failure inside `message`, or null when it is some other failure. */
export function keyLoadFailure(message: string): KeyLoadFailure | null {
  const m = KEY_LOAD.exec(message);
  if (!m) return null;
  const [, path, detail] = m;
  // A .ppk is named first: fixing its path alone would still fail on format.
  if (/\.ppk$/i.test(path)) return { path, reason: 'ppk' };
  if (NOT_FOUND.test(detail)) return { path, reason: 'missing' };
  return { path, reason: 'unreadable' };
}
