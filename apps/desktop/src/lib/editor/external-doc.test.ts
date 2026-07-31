import { describe, it, expect } from 'vitest';
import { ExternalDoc } from './external-doc';

describe('ExternalDoc', () => {
  it('has nothing to apply before anything is pushed', () => {
    const doc = new ExternalDoc();
    expect(doc.hasPending).toBe(false);
    expect(doc.take()).toBeNull();
  });

  it('hands back the pushed document', () => {
    const doc = new ExternalDoc();
    doc.push('SELECT COUNT(*) FROM `t`;');
    expect(doc.hasPending).toBe(true);
    expect(doc.take()).toBe('SELECT COUNT(*) FROM `t`;');
  });

  it('does not hand the same document back twice', () => {
    const doc = new ExternalDoc();
    doc.push('SELECT 1;');
    doc.take();
    expect(doc.hasPending).toBe(false);
    expect(doc.take()).toBeNull();
  });

  it('keeps only the newest of several pushes, since the earlier one was never shown', () => {
    const doc = new ExternalDoc();
    doc.push('SELECT 1;');
    doc.push('SELECT 2;');
    doc.push('SELECT 3;');
    expect(doc.take()).toBe('SELECT 3;');
    expect(doc.take()).toBeNull();
  });

  it('accepts a new push after the previous one was applied', () => {
    const doc = new ExternalDoc();
    doc.push('SELECT 1;');
    doc.take();
    doc.push('SELECT 2;');
    expect(doc.take()).toBe('SELECT 2;');
  });

  it('re-applies the identical text, so running the same menu entry twice resets what you typed over it', () => {
    const doc = new ExternalDoc();
    const sql = 'SELECT COUNT(*) FROM `orders`;';
    doc.push(sql);
    expect(doc.take()).toBe(sql);
    doc.push(sql);
    expect(doc.take()).toBe(sql);
  });

  it('treats an empty string as a real document, not as "nothing pending"', () => {
    const doc = new ExternalDoc();
    doc.push('');
    expect(doc.hasPending).toBe(true);
    expect(doc.take()).toBe('');
  });
});
