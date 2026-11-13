import { Component, OnInit, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { CoreService, errorMessage } from '../core/core.service';

/** Info → Open-source licenses: the license list every bundle carries, as plain text. */
@Component({
  selector: 'app-licenses-page',
  imports: [RouterLink],
  host: { class: 'page' },
  template: `
    <div class="page-inner">
      <a class="back" routerLink="/info"><i class="bi bi-chevron-left"></i> Info</a>
      <h1 class="page-title">Open-source licenses</h1>
      <p class="copy">
        Gooti Desktop is under the MIT License. Below are the licenses of the software it ships with:
        the libraries of the Linux AppImage, the user interface and the Rust crates.
      </p>

      @if (error()) {
        <p class="sam-error" role="alert">{{ error() }}</p>
      } @else if (text() === null) {
        <p class="copy">Loading…</p>
      } @else {
        <pre class="licenses" tabindex="0" aria-label="Licenses">{{ text() }}</pre>
      }
    </div>
  `,
  styles: `
    .back {
      align-self: flex-start;
      color: var(--muted);
      font-size: 14px;
      text-decoration: none;
    }

    .back:hover {
      color: #ffffff;
    }

    .licenses {
      margin: 10px 0 0;
      padding: 14px 16px;
      border-radius: 10px;
      background: rgba(255, 255, 255, 0.04);
      color: var(--muted);
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: 12px;
      line-height: 1.5;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
      user-select: text;
    }
  `,
})
export class LicensesPage implements OnInit {
  readonly #core = inject(CoreService);
  protected readonly text = signal<string | null>(null);
  protected readonly error = signal('');

  async ngOnInit(): Promise<void> {
    try {
      this.text.set(await this.#core.licensesText());
    } catch (error) {
      this.error.set(errorMessage(error));
    }
  }
}
