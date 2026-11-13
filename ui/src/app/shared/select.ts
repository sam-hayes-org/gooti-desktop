import {
  Component,
  ElementRef,
  computed,
  forwardRef,
  input,
  signal,
  viewChild,
} from '@angular/core';
import { ControlValueAccessor, NG_VALUE_ACCESSOR } from '@angular/forms';
import { CdkConnectedOverlay, CdkOverlayOrigin, ConnectedPosition } from '@angular/cdk/overlay';
import { CdkListbox, CdkOption } from '@angular/cdk/listbox';

export interface SelectOption<T> {
  value: T;
  label: string;
  disabled?: boolean;
}

/** Below the field, or above it when there is no room. */
const POSITIONS: ConnectedPosition[] = [
  { originX: 'start', originY: 'bottom', overlayX: 'start', overlayY: 'top', offsetY: 4 },
  { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'bottom', offsetY: -4 },
];

let nextId = 0;

/**
 * A drop-down list drawn by Gooti, not by the system, so it looks the same everywhere.
 * Works with ngModel. Keyboard: Enter, Space or the arrow keys open it; in the list the arrow
 * keys, Home, End and typing move, Enter or Space choose, Escape and Tab close.
 */
@Component({
  selector: 'app-select',
  imports: [CdkOverlayOrigin, CdkConnectedOverlay, CdkListbox, CdkOption],
  providers: [{ provide: NG_VALUE_ACCESSOR, useExisting: forwardRef(() => Select), multi: true }],
  template: `
    <button
      #field
      type="button"
      class="field"
      cdkOverlayOrigin
      #origin="cdkOverlayOrigin"
      role="combobox"
      aria-haspopup="listbox"
      [id]="inputId()"
      [attr.aria-expanded]="open()"
      [attr.aria-controls]="open() ? listId : null"
      [disabled]="disabled()"
      (click)="toggle()"
      (keydown.arrowdown)="show($event)"
      (keydown.arrowup)="show($event)"
      (blur)="touched()"
    >
      <span class="label">{{ selectedLabel() }}</span>
      <i class="bi bi-chevron-down" aria-hidden="true"></i>
    </button>
    <ng-template
      cdkConnectedOverlay
      [cdkConnectedOverlayOrigin]="origin"
      [cdkConnectedOverlayOpen]="open()"
      [cdkConnectedOverlayPositions]="positions"
      [cdkConnectedOverlayMinWidth]="width()"
      (overlayOutsideClick)="close(false)"
      (overlayKeydown)="onOverlayKey($event)"
      (attach)="focusList()"
      (detach)="open() && close(true)"
    >
      <ul
        #list
        cdkListbox
        class="app-select-list"
        [id]="listId"
        [attr.aria-labelledby]="inputId()"
        [cdkListboxValue]="[value()]"
        [cdkListboxCompareWith]="same"
        (cdkListboxValueChange)="pick($event.value)"
      >
        @for (option of options(); track $index) {
          <li class="option" [cdkOption]="option.value" [cdkOptionDisabled]="!!option.disabled">
            <span>{{ option.label }}</span>
            @if (same(option.value, value())) {
              <i class="bi bi-check2" aria-hidden="true"></i>
            }
          </li>
        }
      </ul>
    </ng-template>
  `,
  styles: `
    :host {
      display: inline-block;
      min-width: 0;
    }

    .field {
      display: flex;
      align-items: center;
      gap: 10px;
      width: 100%;
      height: 36px;
      padding: 0 12px;
      border: 1px solid rgba(255, 255, 255, 0.06);
      border-radius: 10px;
      background: var(--background-light);
      color: #ffffff;
      font: inherit;
      font-size: 15px;
      text-align: left;
      cursor: pointer;

      &:hover:not(:disabled) {
        background: var(--background-light-hover);
      }

      &:focus {
        outline: none;
      }

      &:focus-visible,
      &[aria-expanded='true'] {
        border-color: var(--primary);
        box-shadow: 0 0 0 2px rgba(13, 110, 253, 0.35);
      }

      &:disabled {
        opacity: 0.45;
        cursor: default;
      }

      .label {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .bi {
        font-size: 12px;
        color: var(--muted);
      }
    }

    .app-select-list {
      max-height: 280px;
      overflow-y: auto;
      margin: 0;
      padding: 4px;
      list-style: none;
      box-sizing: border-box;
      border: 1px solid rgba(255, 255, 255, 0.08);
      border-radius: 10px;
      background: var(--background-light);
      box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
      color: #ffffff;
      font-size: 15px;

      &:focus {
        outline: none;
      }
    }

    .option {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 7px 10px;
      border-radius: 7px;
      cursor: pointer;
      white-space: nowrap;

      .bi {
        color: var(--primary);
      }

      &:hover:not([aria-disabled='true']),
      &.cdk-option-active {
        background: var(--background-light-hover);
      }

      &:focus {
        outline: none;
      }

      &[aria-disabled='true'] {
        color: var(--muted);
        opacity: 0.6;
        cursor: default;
      }
    }
  `,
})
export class Select<T> implements ControlValueAccessor {
  readonly options = input.required<SelectOption<T>[]>();
  /** The id of the field, for a `<label for>`. */
  readonly inputId = input<string>(`app-select-${nextId}`);

  protected readonly listId = `app-select-list-${nextId++}`;
  protected readonly positions = POSITIONS;
  protected readonly open = signal(false);
  protected readonly value = signal<T | null>(null);
  protected readonly disabled = signal(false);
  protected readonly width = signal(0);
  protected readonly selectedLabel = computed(
    () => this.options().find((o) => this.same(o.value, this.value()))?.label ?? '',
  );

  private readonly field = viewChild.required<ElementRef<HTMLButtonElement>>('field');
  private readonly list = viewChild(CdkListbox);
  #onChange: (value: T) => void = () => undefined;
  #onTouched: () => void = () => undefined;

  protected readonly same = (a: unknown, b: unknown): boolean => a === b;

  writeValue(value: T): void {
    this.value.set(value);
  }

  registerOnChange(fn: (value: T) => void): void {
    this.#onChange = fn;
  }

  registerOnTouched(fn: () => void): void {
    this.#onTouched = fn;
  }

  setDisabledState(disabled: boolean): void {
    this.disabled.set(disabled);
  }

  protected toggle(): void {
    if (this.open()) {
      this.close(true);
    } else {
      this.width.set(this.field().nativeElement.getBoundingClientRect().width);
      this.open.set(true);
    }
  }

  protected show(event: Event): void {
    event.preventDefault();
    if (!this.open()) {
      this.toggle();
    }
  }

  protected focusList(): void {
    // The options render after the overlay is attached. Skip it when the list closed meanwhile.
    setTimeout(() => {
      if (this.open()) {
        this.list()?.focus();
      }
    });
  }

  /** The listbox toggles, so choosing the current option again reports nothing. */
  protected pick(values: readonly (T | null)[]): void {
    if (values.length > 0 && !this.same(values[0], this.value())) {
      const value = values[0] as T;
      this.value.set(value);
      this.#onChange(value);
    }
    this.close(true);
  }

  protected onOverlayKey(event: KeyboardEvent): void {
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      this.close(true);
    }
  }

  protected close(refocus: boolean): void {
    this.open.set(false);
    if (refocus) {
      this.field().nativeElement.focus();
    }
  }

  protected touched(): void {
    if (!this.open()) {
      this.#onTouched();
    }
  }
}
