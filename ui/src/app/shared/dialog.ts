import { Component, input, output } from '@angular/core';

/** A panel over the page, like the extension's modals: title, content, buttons. */
@Component({
  selector: 'app-dialog',
  host: { '(document:keydown.escape)': 'dismissable() && closed.emit()' },
  template: `
    <div class="backdrop" (click)="dismissable() && closed.emit()"></div>
    <div class="dialog" role="dialog" aria-modal="true" [attr.aria-label]="title()">
      <div class="head">
        <h2>{{ title() }}</h2>
        @if (dismissable()) {
          <button type="button" class="close" aria-label="Close" (click)="closed.emit()">
            <i class="bi bi-x-lg"></i>
          </button>
        }
      </div>
      <div class="body"><ng-content /></div>
      <div class="buttons"><ng-content select="[buttons]" /></div>
    </div>
  `,
  styles: `
    :host {
      display: contents;
    }

    .backdrop {
      position: fixed;
      inset: 0;
      z-index: 90;
      background: rgba(0, 0, 0, 0.55);
    }

    .dialog {
      position: fixed;
      z-index: 91;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      width: min(480px, calc(100% - 32px));
      max-height: calc(100% - 64px);
      display: flex;
      flex-direction: column;
      box-sizing: border-box;
      border-radius: 14px;
      border: 1px solid rgba(255, 255, 255, 0.06);
      background: var(--background-light);
      box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
    }

    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 18px 20px 0;

      h2 {
        margin: 0;
        font-size: 18px;
        font-weight: 600;
      }
    }

    .close {
      border: 0;
      background: transparent;
      color: var(--muted);
      cursor: pointer;

      &:hover {
        color: #ffffff;
      }
    }

    .body {
      display: flex;
      flex-direction: column;
      gap: 10px;
      padding: 14px 20px;
      overflow-y: auto;
    }

    .buttons {
      display: flex;
      justify-content: flex-end;
      gap: 8px;
      padding: 0 20px 18px;
    }
  `,
})
export class Dialog {
  readonly title = input.required<string>();
  /** False while something runs that must not be left. */
  readonly dismissable = input(true);
  readonly closed = output<void>();
}
