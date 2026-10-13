/** Main owns publication across every window, before any asynchronous receipt/media read. */
export class BenchPublicationOwner {
  private running = false;

  async run<T>(operation: () => Promise<T>): Promise<T | { ok: false; error: string }> {
    if (this.running)
      return { ok: false, error: 'A benchmark publication is already in progress.' };
    this.running = true;
    try {
      return await operation();
    } finally {
      this.running = false;
    }
  }
}
