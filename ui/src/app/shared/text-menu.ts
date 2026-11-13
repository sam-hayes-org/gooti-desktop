import {
  Component,
  Injector,
  OnDestroy,
  TemplateRef,
  ViewContainerRef,
  inject,
  isDevMode,
  signal,
  viewChild,
} from '@angular/core';
import { CdkMenu, CdkMenuItem } from '@angular/cdk/menu';
import {
  OverlayRef,
  createCloseScrollStrategy,
  createFlexibleConnectedPositionStrategy,
  createOverlayRef,
} from '@angular/cdk/overlay';
import { TemplatePortal } from '@angular/cdk/portal';
import { isTauri } from '@tauri-apps/api/core';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';

type Field = HTMLInputElement | HTMLTextAreaElement;
type Action = 'cut' | 'copy' | 'paste' | 'selectAll';

const TEXT_INPUTS = new Set(['text', 'password', 'search', 'url', 'email', 'tel', 'number', '']);

/** Text fields, and only those, get the menu. */
export function textField(target: EventTarget | null): Field | null {
  if (target instanceof HTMLTextAreaElement) {
    return target;
  }
  if (target instanceof HTMLInputElement && TEXT_INPUTS.has(target.type)) {
    return target;
  }
  return null;
}

async function clipboardWrite(text: string): Promise<void> {
  if (isTauri()) {
    await writeText(text);
  } else {
    await navigator.clipboard.writeText(text);
  }
}

async function clipboardRead(): Promise<string> {
  return isTauri() ? readText() : navigator.clipboard.readText();
}

/**
 * Replaces the system's right-click menu. Text fields get Cut, Copy, Paste and Select all, and
 * selected text elsewhere gets Copy. Anywhere else nothing opens. In development builds,
 * Shift + right-click still shows the system menu (with Inspect).
 */
@Component({
  selector: 'app-text-menu',
  imports: [CdkMenu, CdkMenuItem],
  host: { '(document:contextmenu)': 'onContextMenu($event)' },
  template: `
    <ng-template #menu>
      <div cdkMenu class="text-menu" aria-label="Edit">
        @if (field()) {
          <button type="button" cdkMenuItem [cdkMenuItemDisabled]="!canCut()" [disabled]="!canCut()" (cdkMenuItemTriggered)="run('cut')">
            Cut
          </button>
        }
        <button type="button" cdkMenuItem [cdkMenuItemDisabled]="!canCopy()" [disabled]="!canCopy()" (cdkMenuItemTriggered)="run('copy')">
          Copy
        </button>
        @if (field()) {
          <button type="button" cdkMenuItem [cdkMenuItemDisabled]="!canPaste()" [disabled]="!canPaste()" (cdkMenuItemTriggered)="run('paste')">
            Paste
          </button>
          <div class="separator" role="separator"></div>
          <button type="button" cdkMenuItem (cdkMenuItemTriggered)="run('selectAll')">Select all</button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    /* No box of its own: in the app's grid an empty element would still take a row. */
    :host {
      display: contents;
    }

    .text-menu {
      display: flex;
      flex-direction: column;
      min-width: 160px;
      padding: 4px;
      border: 1px solid rgba(255, 255, 255, 0.08);
      border-radius: 10px;
      background: var(--background-light);
      box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);

      &:focus {
        outline: none;
      }
    }

    button {
      padding: 6px 10px;
      border: 0;
      border-radius: 7px;
      background: transparent;
      color: #ffffff;
      font: inherit;
      font-size: 14px;
      text-align: left;
      cursor: pointer;

      &:hover:not(:disabled),
      &:focus {
        outline: none;
        background: var(--background-light-hover);
      }

      &:disabled {
        color: var(--muted);
        opacity: 0.6;
        cursor: default;
      }
    }

    .separator {
      height: 1px;
      margin: 4px 6px;
      background: rgba(255, 255, 255, 0.08);
    }
  `,
})
export class TextMenu implements OnDestroy {
  readonly #injector = inject(Injector);
  readonly #viewContainer = inject(ViewContainerRef);
  private readonly template = viewChild.required<TemplateRef<unknown>>('menu');
  #overlay: OverlayRef | null = null;
  /** The selection when the menu opened; focus moves into the menu after that. */
  #start = 0;
  #end = 0;
  #pageText = '';

  protected readonly field = signal<Field | null>(null);
  protected readonly canCut = signal(false);
  protected readonly canCopy = signal(false);
  protected readonly canPaste = signal(false);

  onContextMenu(event: MouseEvent): void {
    if (isDevMode() && event.shiftKey) {
      return;
    }
    event.preventDefault();
    this.close();

    const field = textField(event.target);
    if (field) {
      if (field.disabled) {
        return;
      }
      this.#start = field.selectionStart ?? 0;
      this.#end = field.selectionEnd ?? 0;
      const selected = this.#end > this.#start;
      // Like browsers: a password can be pasted but not copied out.
      const secret = field instanceof HTMLInputElement && field.type === 'password';
      this.field.set(field);
      this.canCopy.set(selected && !secret);
      this.canCut.set(selected && !secret && !field.readOnly);
      this.canPaste.set(!field.readOnly);
    } else {
      this.#pageText = window.getSelection()?.toString() ?? '';
      if (!this.#pageText) {
        return;
      }
      this.field.set(null);
      this.canCopy.set(true);
    }
    this.#open(event.clientX, event.clientY);
  }

  #open(x: number, y: number): void {
    const position = createFlexibleConnectedPositionStrategy(this.#injector, { x, y })
      .withPositions([
        { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'top' },
        { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'bottom' },
        { originX: 'start', originY: 'top', overlayX: 'end', overlayY: 'top' },
        { originX: 'start', originY: 'top', overlayX: 'end', overlayY: 'bottom' },
      ])
      .withViewportMargin(8);
    const overlay = createOverlayRef(this.#injector, {
      positionStrategy: position,
      scrollStrategy: createCloseScrollStrategy(this.#injector),
    });
    overlay.outsidePointerEvents().subscribe(() => this.close());
    overlay.keydownEvents().subscribe((key) => {
      if (key.key === 'Escape' || key.key === 'Tab') {
        key.preventDefault();
        this.close(true);
      }
    });
    overlay.attach(new TemplatePortal(this.template(), this.#viewContainer));
    this.#overlay = overlay;
    setTimeout(() => overlay.overlayElement?.querySelector<HTMLElement>('[cdkMenuItem]:not(:disabled)')?.focus());
  }

  protected async run(action: Action): Promise<void> {
    const field = this.field();
    this.close(true);
    if (!field) {
      if (action === 'copy') {
        await clipboardWrite(this.#pageText);
      }
      return;
    }
    const selected = field.value.slice(this.#start, this.#end);
    switch (action) {
      case 'copy':
        await clipboardWrite(selected);
        break;
      case 'cut':
        await clipboardWrite(selected);
        replace(field, '', this.#start, this.#end);
        break;
      case 'paste':
        replace(field, await clipboardRead(), this.#start, this.#end);
        break;
      case 'selectAll':
        field.select();
        break;
    }
  }

  close(refocus = false): void {
    this.#overlay?.dispose();
    this.#overlay = null;
    const field = this.field();
    if (refocus && field) {
      field.focus();
      field.setSelectionRange(this.#start, this.#end);
    }
  }

  ngOnDestroy(): void {
    this.#overlay?.dispose();
  }
}

/** Replaces part of a field's text and tells Angular forms, as typing would. */
function replace(field: Field, text: string, start: number, end: number): void {
  field.focus();
  field.setRangeText(text, start, end, 'end');
  field.dispatchEvent(new Event('input', { bubbles: true }));
}
