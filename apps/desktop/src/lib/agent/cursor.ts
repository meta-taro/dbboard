// The agent's own cursor inside the window (ADR-0177). It never touches the
// OS pointer or keyboard: it draws an arrow over the page, outlines what it
// points at, and presses things by calling them from inside the page. The
// person can keep using their own mouse in the same window meanwhile.
//
// No framework and no `$lib` imports, so the end-to-end tests can bundle this
// file alone and drive the very code the app runs.
import { placeBubble, showMs } from './place';
import type { ScreenStep } from './step';

/** How long the cursor takes to glide to a target. Matches the CSS below. */
export const MOVE_MS = 650;
/** The attribute that makes a button a person's act. */
export const REFUSE_ATTR = 'data-agent-refuse';

/** What can be pointed at, clicked or typed into. */
const CANDIDATES =
  'button, a[href], input, select, textarea, label, th, [role="tab"], [role="menuitem"], [role="button"], h1, h2, h3';

const STYLE = `
.agent-cursor{position:fixed;left:0;top:0;z-index:2147483647;pointer-events:none;display:flex;align-items:flex-start;gap:2px;transition:transform ${MOVE_MS}ms cubic-bezier(.2,.8,.2,1);will-change:transform}
.agent-cursor svg{width:22px;height:22px;filter:drop-shadow(0 1px 2px rgba(0,0,0,.35))}
.agent-cursor .tag{margin-top:14px;padding:1px 6px;border-radius:999px;background:#5e6ad2;color:#fff;font:600 11px/1.6 system-ui,sans-serif;white-space:nowrap}
.agent-box{position:fixed;z-index:2147483646;pointer-events:none;border:2px solid #5e6ad2;border-radius:6px;box-shadow:0 0 0 4px rgba(94,106,210,.25);transition:all ${MOVE_MS}ms cubic-bezier(.2,.8,.2,1)}
.agent-ring{position:fixed;z-index:2147483646;pointer-events:none;width:28px;height:28px;margin:-14px 0 0 -14px;border:2px solid #5e6ad2;border-radius:50%;animation:agent-ring .6s ease-out forwards}
@keyframes agent-ring{from{transform:scale(.4);opacity:1}to{transform:scale(1.6);opacity:0}}
.agent-bubble{position:fixed;z-index:2147483647;pointer-events:none;max-width:320px;padding:8px 12px;border-radius:10px;background:#ffd166;color:#1f1f1f;font:13px/1.5 system-ui,sans-serif;box-shadow:0 4px 14px rgba(0,0,0,.25);white-space:pre-wrap;overflow-wrap:anywhere}
.agent-bubble::after{content:"";position:absolute;width:12px;height:12px;background:#ffd166;transform:rotate(45deg)}
.agent-bubble[data-tail=top]::after{top:-6px;left:calc(50% - 6px)}
.agent-bubble[data-tail=bottom]::after{bottom:-6px;left:calc(50% - 6px)}
.agent-bubble[data-tail=left]::after{left:-6px;top:calc(50% - 6px)}
.agent-bubble[data-tail=right]::after{right:-6px;top:calc(50% - 6px)}
`;

const ARROW =
  '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 2l15 9.5-6.6 1.4L16 20.5l-3 1.3-3.5-7.6L4 18z" fill="#5e6ad2" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/></svg>';

const normalise = (s: string | null | undefined) => (s ?? '').replace(/\s+/g, ' ').trim().toLowerCase();

function isVisible(el: Element): boolean {
  const r = el.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) return false;
  const style = getComputedStyle(el);
  return style.visibility !== 'hidden' && style.display !== 'none';
}

/** Every name a person could call `el` by. */
function namesOf(el: Element): string[] {
  const names = [
    (el as HTMLElement).innerText,
    el.getAttribute('aria-label'),
    el.getAttribute('title'),
    el.getAttribute('placeholder'),
  ];
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
    for (const label of Array.from(el.labels ?? [])) names.push(label.innerText);
  }
  return names.map(normalise).filter(Boolean);
}

/**
 * The element on screen that `words` name, or null. An exact name wins over
 * one that starts with the words, which wins over one that contains them; in
 * a tie, the first in reading order. The cursor's own parts never match.
 */
export function findTarget(root: ParentNode, words: string): HTMLElement | null {
  const want = normalise(words);
  if (!want) return null;
  let best: { el: HTMLElement; rank: number } | null = null;
  for (const el of Array.from(root.querySelectorAll<HTMLElement>(CANDIDATES))) {
    if (el.closest('[data-agent-overlay]') || !isVisible(el)) continue;
    const names = namesOf(el);
    const rank = names.includes(want)
      ? 0
      : names.some((n) => n.startsWith(want))
        ? 1
        : names.some((n) => n.includes(want))
          ? 2
          : -1;
    if (rank >= 0 && (!best || rank < best.rank)) best = { el, rank };
    if (best?.rank === 0) break;
  }
  return best?.el ?? null;
}

/** A label stands for its field: typing "Search tables" means its input. */
function fieldOf(el: HTMLElement): HTMLInputElement | HTMLTextAreaElement | null {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) return el;
  if (el instanceof HTMLLabelElement) {
    const control = el.control;
    if (control instanceof HTMLInputElement || control instanceof HTMLTextAreaElement) return control;
  }
  return el.querySelector('input, textarea');
}

/** Set a field the way typing would, so frameworks see the change. */
function fill(field: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  const proto = field instanceof HTMLInputElement ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(field, value);
  field.dispatchEvent(new Event('input', { bubbles: true }));
  field.dispatchEvent(new Event('change', { bubbles: true }));
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export interface CursorOptions {
  /** The words on the cursor's tag. */
  tag: string;
  /** Switch to the tab a `go` names; false when there is no such tab. */
  goTo?: (hash: string) => boolean;
  /** Shorten every wait, for tests. */
  fast?: boolean;
}

export class AgentCursor {
  #doc: Document;
  #opts: CursorOptions;
  #cursor: HTMLElement | null = null;
  #box: HTMLElement | null = null;
  #bubble: HTMLElement | null = null;
  #hideTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(doc: Document, opts: CursorOptions) {
    this.#doc = doc;
    this.#opts = opts;
  }

  /** Carry out one step and say what happened, or throw the reason it could not. */
  async perform(step: ScreenStep): Promise<string> {
    if (step.action === 'say') {
      this.#show(null, step.message, step.seconds);
      return 'said it';
    }
    if (step.action === 'go') {
      if (!this.#opts.goTo?.(step.hash)) {
        throw new Error(`there is no tab ${step.hash} in this window right now`);
      }
      this.#show(null, step.message, step.seconds);
      return `switched to ${step.hash}`;
    }
    const el = findTarget(this.#doc, step.target);
    if (!el) {
      throw new Error(`nothing on screen matches "${step.target}"; nothing was pressed`);
    }
    await this.#moveTo(el);
    this.#show(el, step.message, step.seconds);
    const name = step.target;
    if (step.action === 'point') return `pointed at "${name}"`;
    if (step.action === 'click') {
      if (el.closest(`[${REFUSE_ATTR}]`)) {
        throw new Error(
          `"${name}" is a person's button, so it was not pressed; point at it and ask the person to press it`,
        );
      }
      this.#ring(el);
      el.click();
      return `clicked "${name}"`;
    }
    const field = fieldOf(el);
    if (!field || field.disabled || field.readOnly) {
      throw new Error(`"${name}" is not a field that can be typed into`);
    }
    field.focus({ preventScroll: true });
    fill(field, step.value);
    return `typed ${[...step.value].length} characters into "${name}"`;
  }

  /** Remove everything the cursor drew. */
  dispose(): void {
    clearTimeout(this.#hideTimer);
    for (const el of [this.#cursor, this.#box, this.#bubble]) el?.remove();
    this.#cursor = this.#box = this.#bubble = null;
  }

  #ensure(): { cursor: HTMLElement; box: HTMLElement; bubble: HTMLElement } {
    if (this.#cursor && this.#box && this.#bubble) {
      return { cursor: this.#cursor, box: this.#box, bubble: this.#bubble };
    }
    const doc = this.#doc;
    if (!doc.getElementById('agent-cursor-style')) {
      const style = doc.createElement('style');
      style.id = 'agent-cursor-style';
      style.textContent = STYLE;
      doc.head.appendChild(style);
    }
    const make = (cls: string) => {
      const el = doc.createElement('div');
      el.className = cls;
      el.setAttribute('data-agent-overlay', '');
      el.hidden = true;
      doc.body.appendChild(el);
      return el;
    };
    const cursor = make('agent-cursor');
    cursor.setAttribute('aria-hidden', 'true');
    cursor.innerHTML = ARROW;
    const tag = doc.createElement('span');
    tag.className = 'tag';
    tag.textContent = this.#opts.tag;
    cursor.appendChild(tag);
    const box = make('agent-box');
    const bubble = make('agent-bubble');
    bubble.setAttribute('role', 'status');
    this.#cursor = cursor;
    this.#box = box;
    this.#bubble = bubble;
    return { cursor, box, bubble };
  }

  async #moveTo(el: HTMLElement): Promise<void> {
    const { cursor, box } = this.#ensure();
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    const r = el.getBoundingClientRect();
    cursor.hidden = false;
    cursor.style.transform = `translate(${r.left + r.width / 2}px, ${r.top + r.height / 2}px)`;
    Object.assign(box.style, {
      left: `${r.left - 4}px`,
      top: `${r.top - 4}px`,
      width: `${r.width + 8}px`,
      height: `${r.height + 8}px`,
    });
    box.hidden = false;
    await sleep(this.#opts.fast ? 0 : MOVE_MS);
  }

  #ring(el: HTMLElement): void {
    const r = el.getBoundingClientRect();
    const ring = this.#doc.createElement('div');
    ring.className = 'agent-ring';
    ring.setAttribute('data-agent-overlay', '');
    ring.style.left = `${r.left + r.width / 2}px`;
    ring.style.top = `${r.top + r.height / 2}px`;
    this.#doc.body.appendChild(ring);
    setTimeout(() => ring.remove(), 700);
  }

  #show(target: HTMLElement | null, message: string | undefined, seconds?: number): void {
    const { box, bubble } = this.#ensure();
    clearTimeout(this.#hideTimer);
    if (message) {
      bubble.textContent = message;
      bubble.hidden = false;
      const view = this.#doc.defaultView;
      const placed = placeBubble(
        target?.getBoundingClientRect() ?? null,
        { width: bubble.offsetWidth, height: bubble.offsetHeight },
        { width: view?.innerWidth ?? 0, height: view?.innerHeight ?? 0 },
      );
      bubble.style.left = `${placed.left}px`;
      bubble.style.top = `${placed.top}px`;
      bubble.dataset.tail = placed.tail;
    } else {
      bubble.hidden = true;
    }
    this.#hideTimer = setTimeout(
      () => {
        box.hidden = true;
        bubble.hidden = true;
      },
      showMs(message, seconds),
    );
  }
}
