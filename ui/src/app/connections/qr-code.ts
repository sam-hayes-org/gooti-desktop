import { Component, effect, input, signal } from '@angular/core';
import { DomSanitizer, SafeHtml } from '@angular/platform-browser';
import { inject } from '@angular/core';
import QRCode from 'qrcode';

/** A QR code as inline SVG: dark modules on white, so phone cameras read it. */
@Component({
  selector: 'app-qr-code',
  template: `<div class="qr" role="img" [attr.aria-label]="label()" [innerHTML]="svg()"></div>`,
  styles: `
    .qr {
      width: 200px;
      height: 200px;
      padding: 10px;
      border-radius: 12px;
      background: #ffffff;
      box-sizing: border-box;
    }

    .qr ::ng-deep svg {
      display: block;
      width: 100%;
      height: 100%;
    }
  `,
})
export class QrCode {
  readonly text = input.required<string>();
  readonly label = input('QR code');
  readonly #sanitizer = inject(DomSanitizer);
  protected readonly svg = signal<SafeHtml>('');

  constructor() {
    effect(() => {
      const text = this.text();
      void QRCode.toString(text, { type: 'svg', margin: 0, errorCorrectionLevel: 'M' }).then((svg) =>
        // The SVG comes from the qrcode library, not from the link text.
        this.svg.set(this.#sanitizer.bypassSecurityTrustHtml(svg)),
      );
    });
  }
}
