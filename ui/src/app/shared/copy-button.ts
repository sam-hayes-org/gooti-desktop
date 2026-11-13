import { Component, input, signal } from '@angular/core';

/** Copies `text` to the clipboard and says so for a moment. */
@Component({
  selector: 'app-copy-button',
  template: `
    <button type="button" class="icon" [attr.aria-label]="copied() ? 'Copied' : 'Copy'" (click)="copy()">
      <i class="bi" [class.bi-copy]="!copied()" [class.bi-check2]="copied()"></i>
    </button>
  `,
  styles: `
    :host {
      display: contents;
    }
  `,
})
export class CopyButton {
  readonly text = input.required<string>();
  protected readonly copied = signal(false);

  async copy(): Promise<void> {
    await navigator.clipboard.writeText(this.text());
    this.copied.set(true);
    setTimeout(() => this.copied.set(false), 1500);
  }
}
