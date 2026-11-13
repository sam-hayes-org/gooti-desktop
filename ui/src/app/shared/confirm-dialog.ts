import { Component, inject } from '@angular/core';
import { ConfirmService } from './confirm.service';

@Component({
  selector: 'app-confirm-dialog',
  host: { '(document:keydown.escape)': 'confirm.answer(false)' },
  template: `
    @if (confirm.pending(); as request) {
      <div class="backdrop" (click)="confirm.answer(false)"></div>
      <div class="dialog" role="alertdialog" aria-modal="true" [attr.aria-label]="request.message">
        <p class="message">{{ request.message }}</p>
        <div class="buttons">
          <button type="button" class="action quiet" (click)="confirm.answer(false)">No</button>
          <button
            type="button"
            class="action"
            [class.primary]="!request.danger"
            [class.danger]="request.danger"
            (click)="confirm.answer(true)"
          >
            {{ request.yesLabel }}
          </button>
        </div>
      </div>
    }
  `,
  styles: `
    /* No box of its own, so it never takes a cell in the app's grid. */
    :host {
      display: contents;
    }

    .backdrop {
      position: fixed;
      inset: 0;
      z-index: 100;
      background: rgba(0, 0, 0, 0.55);
    }

    .dialog {
      position: fixed;
      z-index: 101;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      width: min(380px, calc(100% - 32px));
      box-sizing: border-box;
      padding: 20px;
      border-radius: 14px;
      border: 1px solid rgba(255, 255, 255, 0.06);
      background: var(--background-light);
      box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
    }

    .message {
      margin: 0;
      font-size: 16px;
      line-height: 1.4;
    }

    .buttons {
      display: flex;
      justify-content: flex-end;
      gap: 8px;
      margin-top: 20px;
    }
  `,
})
export class ConfirmDialog {
  protected readonly confirm = inject(ConfirmService);
}
