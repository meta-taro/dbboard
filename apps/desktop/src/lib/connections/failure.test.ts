import { describe, expect, it } from 'vitest';
import { keyLoadFailure } from './failure';

const wrap = (path: string, detail: string) =>
  `connection failed: ssh tunnel: failed to load ssh private key '${path}': ${detail}`;

describe('keyLoadFailure', () => {
  it('is null for anything that is not a key-load failure', () => {
    // Reconnect stays the right answer for a dropped tunnel or a refused login.
    expect(keyLoadFailure('connection failed: ssh connect to bastion:22 failed')).toBeNull();
    expect(keyLoadFailure("ssh authentication failed for user 'deploy'")).toBeNull();
    expect(keyLoadFailure('')).toBeNull();
  });

  it('says the file is missing when the OS said so (Unix)', () => {
    expect(keyLoadFailure(wrap('/home/a/.ssh/id_ed25519', 'No such file or directory (os error 2)'))).toEqual({
      path: '/home/a/.ssh/id_ed25519',
      reason: 'missing',
    });
  });

  it('says the file is missing when the OS said so (Windows)', () => {
    const msg = wrap(
      'C:\\Users\\a\\keys\\bastion',
      'The system cannot find the path specified. (os error 3)',
    );
    expect(keyLoadFailure(msg)).toEqual({ path: 'C:\\Users\\a\\keys\\bastion', reason: 'missing' });
  });

  it('names a PuTTY key as the problem even before the file is checked', () => {
    // A .ppk is never read, so repairing the path alone would fail again.
    expect(keyLoadFailure(wrap('C:\\keys\\bastion.PPK', 'No such file or directory (os error 2)'))).toEqual({
      path: 'C:\\keys\\bastion.PPK',
      reason: 'ppk',
    });
  });

  it('calls anything else an unreadable key', () => {
    expect(keyLoadFailure(wrap('/k/id_rsa', 'the passphrase is wrong'))).toEqual({
      path: '/k/id_rsa',
      reason: 'unreadable',
    });
  });

  it('keeps a quote inside the path', () => {
    expect(keyLoadFailure(wrap("/k/o'brien/id", 'No such file or directory (os error 2)'))?.path).toBe(
      "/k/o'brien/id",
    );
  });
});
