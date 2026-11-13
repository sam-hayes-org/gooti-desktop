import { Component, computed, input } from '@angular/core';
import { avatarCells, colorForPubkey } from './appearance';

/** Same identicon as the extension's `lib-identity-avatar`. */
@Component({
  selector: 'app-identity-avatar',
  host: {
    '[style.width.px]': 'size()',
    '[style.height.px]': 'size()',
    '[style.--identity-color]': 'fill()',
  },
  template: `
    <svg viewBox="0 0 5 5" aria-hidden="true">
      @for (cell of cells(); track cell.x + '-' + cell.y) {
        <rect [attr.x]="cell.x" [attr.y]="cell.y" width="1" height="1" [attr.fill]="fill()" />
      }
    </svg>
  `,
  styles: `
    :host {
      display: block;
      flex: 0 0 auto;
      border-radius: 8px;
      overflow: hidden;
      background: color-mix(in srgb, var(--identity-color, var(--primary)) 22%, var(--background));
    }

    svg {
      display: block;
      width: 100%;
      height: 100%;
    }
  `,
})
export class IdentityAvatar {
  readonly pubkey = input.required<string>();
  readonly color = input<string | null>(null);
  readonly size = input(28);

  protected readonly fill = computed(() => colorForPubkey(this.pubkey(), this.color()));
  protected readonly cells = computed(() => avatarCells(this.pubkey()));
}
