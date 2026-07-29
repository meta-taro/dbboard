import { describe, it, expect } from 'vitest';
import {
  normalizeVersion,
  parseVersion,
  isNewer,
  emptyDownload,
  foldDownload,
  downloadPercent,
  type DownloadState,
} from './notice';

describe('normalizeVersion', () => {
  it('strips a leading v/V and surrounding whitespace', () => {
    expect(normalizeVersion('v0.4.0')).toBe('0.4.0');
    expect(normalizeVersion('V3.1.5')).toBe('3.1.5');
    expect(normalizeVersion(' 0.4.0 ')).toBe('0.4.0');
    expect(normalizeVersion('0.4.0')).toBe('0.4.0');
  });
});

describe('parseVersion', () => {
  it('accepts a v prefix and fills missing components with 0', () => {
    expect(parseVersion('v0.4.0')).toEqual({ major: 0, minor: 4, patch: 0 });
    expect(parseVersion('0.4.0')).toEqual({ major: 0, minor: 4, patch: 0 });
    expect(parseVersion('1.4')).toEqual({ major: 1, minor: 4, patch: 0 });
    expect(parseVersion('2')).toEqual({ major: 2, minor: 0, patch: 0 });
    expect(parseVersion(' V3.1.5 ')).toEqual({ major: 3, minor: 1, patch: 5 });
  });

  it('drops pre-release and build metadata', () => {
    expect(parseVersion('0.4.0-rc1')).toEqual({ major: 0, minor: 4, patch: 0 });
    expect(parseVersion('1.0.0+build.7')).toEqual({
      major: 1,
      minor: 0,
      patch: 0,
    });
  });

  it('rejects a non-numeric or empty core', () => {
    expect(parseVersion('latest')).toBeNull();
    expect(parseVersion('v')).toBeNull();
    expect(parseVersion('')).toBeNull();
    expect(parseVersion('0.x.0')).toBeNull();
  });
});

describe('isNewer', () => {
  it('is true when latest exceeds current in any component', () => {
    expect(isNewer('0.3.0', '0.4.0')).toBe(true);
    expect(isNewer('0.4.0', '0.4.1')).toBe(true);
    expect(isNewer('0.9.9', '1.0.0')).toBe(true);
    expect(isNewer('0.4.0', 'v0.4.1')).toBe(true);
  });

  it('is false for equal or older', () => {
    expect(isNewer('0.4.0', '0.4.0')).toBe(false);
    expect(isNewer('0.4.0', 'v0.4.0')).toBe(false);
    expect(isNewer('0.4.0', '0.3.9')).toBe(false);
    expect(isNewer('1.0.0', '0.9.9')).toBe(false);
  });

  it('is false when either side is unparseable (never a phantom update)', () => {
    expect(isNewer('0.4.0', 'not-a-version')).toBe(false);
    expect(isNewer('garbage', '9.9.9')).toBe(false);
  });
});

describe('foldDownload', () => {
  it('starts with an unknown total and zero downloaded', () => {
    expect(emptyDownload()).toEqual({ downloaded: 0, total: null });
  });

  it('Started sets the total and resets the counter', () => {
    const s = foldDownload(emptyDownload(), {
      event: 'Started',
      data: { contentLength: 2048 },
    });
    expect(s).toEqual({ downloaded: 0, total: 2048 });
  });

  it('Started with no length leaves the total unknown', () => {
    const s = foldDownload(emptyDownload(), { event: 'Started', data: {} });
    expect(s).toEqual({ downloaded: 0, total: null });
  });

  it('Progress accumulates chunk lengths without mutating', () => {
    const start: DownloadState = { downloaded: 0, total: 1000 };
    const a = foldDownload(start, { event: 'Progress', data: { chunkLength: 400 } });
    const b = foldDownload(a, { event: 'Progress', data: { chunkLength: 250 } });
    expect(a).toEqual({ downloaded: 400, total: 1000 });
    expect(b).toEqual({ downloaded: 650, total: 1000 });
    expect(start.downloaded).toBe(0); // original untouched
  });

  it('Finished leaves the totals as-is', () => {
    const s: DownloadState = { downloaded: 1000, total: 1000 };
    expect(foldDownload(s, { event: 'Finished' })).toEqual(s);
  });
});

describe('downloadPercent', () => {
  it('is a rounded 0..100 percentage when the total is known', () => {
    expect(downloadPercent({ downloaded: 0, total: 1000 })).toBe(0);
    expect(downloadPercent({ downloaded: 650, total: 1000 })).toBe(65);
    expect(downloadPercent({ downloaded: 1000, total: 1000 })).toBe(100);
  });

  it('clamps an overshoot to 100', () => {
    expect(downloadPercent({ downloaded: 1200, total: 1000 })).toBe(100);
  });

  it('is null when the total is unknown or zero (indeterminate)', () => {
    expect(downloadPercent({ downloaded: 500, total: null })).toBeNull();
    expect(downloadPercent({ downloaded: 0, total: 0 })).toBeNull();
  });
});
