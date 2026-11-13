import { Injectable, signal } from '@angular/core';

export interface ConfirmRequest {
  message: string;
  yesLabel: string;
  danger: boolean;
  resolve: (yes: boolean) => void;
}

/** Short questions such as "Lock Gooti?". One at a time. */
@Injectable({ providedIn: 'root' })
export class ConfirmService {
  readonly pending = signal<ConfirmRequest | null>(null);

  ask(message: string, options: { yesLabel?: string; danger?: boolean } = {}): Promise<boolean> {
    this.pending()?.resolve(false);
    return new Promise((resolve) => {
      this.pending.set({
        message,
        yesLabel: options.yesLabel ?? 'Yes',
        danger: options.danger ?? false,
        resolve,
      });
    });
  }

  answer(yes: boolean): void {
    const request = this.pending();
    this.pending.set(null);
    request?.resolve(yes);
  }
}
