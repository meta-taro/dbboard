// How long a real connection made you wait (docs/startup-measurement.md,
// item 2): picking a connection until its table list is on screen, and Run
// until the first row is.
//
// Item 2 was stuck on two things only a person could supply — which
// connection to use, and being there to time it. Measuring inside the app
// removes both: every ordinary session against a real database leaves a
// number in About, and nobody has to stage one.
//
// **Measured to the paint, not to the reply.** The status bar already shows
// how long the statement took; that stops when the IPC call returns, before a
// row has been drawn. This stops two animation frames later, the same
// definition of "on screen" as first paint (ADR-0156), because what a person
// waits for is the rows, not the promise.
//
// The engine is kept with each sample and the connection's name is not: the
// kind is what makes two numbers comparable, and a name in a screenshot of
// About is a name in a bug report (ADR-0055).

type Raf = (cb: () => void) => void;

export interface Timed<T> {
  value?: T;
  error?: unknown;
  failed: boolean;
  ms: number;
}

export interface LatencySample {
  ms: number;
  failed: boolean;
  kind: string;
}

/**
 * Run `work`, then wait until its result has been presented, and say how
 * long that took. A failure is timed as well and handed back rather than
 * thrown, so the caller decides what an error means.
 */
export async function untilPainted<T>(
  work: () => Promise<T>,
  deps: { now?: () => number; raf?: Raf } = {},
): Promise<Timed<T>> {
  const now = deps.now ?? (() => performance.now());
  const raf = deps.raf ?? ((cb) => requestAnimationFrame(cb));
  const began = now();
  let result: Omit<Timed<T>, 'ms'>;
  try {
    result = { value: await work(), failed: false };
  } catch (error) {
    result = { error, failed: true };
  }
  await new Promise<void>((resolve) => raf(() => raf(() => resolve())));
  return { ...result, ms: now() - began };
}

export class Latency {
  /** Choosing a connection until its tables are listed. Includes opening the
   *  connection when this is its first use — an SSH tunnel, an IAM token —
   *  which is the part a person actually notices. */
  tables = $state<LatencySample | null>(null);

  /** Run until the result grid has painted. */
  firstRow = $state<LatencySample | null>(null);

  recordTables(timed: { ms: number; failed: boolean }, kind: string): void {
    this.tables = sample(timed, kind);
  }

  recordFirstRow(timed: { ms: number; failed: boolean }, kind: string): void {
    this.firstRow = sample(timed, kind);
  }
}

function sample(timed: { ms: number; failed: boolean }, kind: string): LatencySample {
  return { ms: Math.round(timed.ms), failed: timed.failed, kind };
}

export const latency = new Latency();
