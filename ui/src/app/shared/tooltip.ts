import {
  Component,
  Directive,
  ElementRef,
  Injector,
  OnDestroy,
  inject,
  input,
  signal,
} from '@angular/core';
import {
  OverlayRef,
  createFlexibleConnectedPositionStrategy,
  createOverlayRef,
  createRepositionScrollStrategy,
} from '@angular/cdk/overlay';
import { ComponentPortal } from '@angular/cdk/portal';

const SHOW_AFTER_MS = 400;
let nextId = 0;

@Component({
  selector: 'app-tooltip-bubble',
  host: { role: 'tooltip', '[id]': 'id' },
  template: `{{ text }}`,
  styles: `
    :host {
      display: block;
      max-width: 280px;
      padding: 6px 10px;
      border: 1px solid rgba(255, 255, 255, 0.08);
      border-radius: 8px;
      background: #2b3240;
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
      color: #ffffff;
      font-size: 13px;
      line-height: 1.4;
      pointer-events: none;
    }
  `,
})
export class TooltipBubble {
  id = '';
  text = '';
}

/** `appTooltip="..."`: a hint drawn by Gooti on hover or keyboard focus, in place of `title`. */
@Directive({
  selector: '[appTooltip]',
  host: {
    '(mouseenter)': 'later()',
    '(focusin)': 'later()',
    '(mouseleave)': 'hide()',
    '(focusout)': 'hide()',
    '(document:keydown.escape)': 'hide()',
    '[attr.aria-describedby]': 'describedBy()',
  },
})
export class Tooltip implements OnDestroy {
  readonly appTooltip = input.required<string>();

  readonly #host = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly #injector = inject(Injector);
  readonly #id = `app-tooltip-${nextId++}`;
  #overlay: OverlayRef | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  protected readonly describedBy = signal<string | null>(null);

  protected later(): void {
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.show(), SHOW_AFTER_MS);
  }

  show(): void {
    if (this.#overlay?.hasAttached() || !this.appTooltip()) {
      return;
    }
    const position = createFlexibleConnectedPositionStrategy(this.#injector, this.#host)
      .withPositions([
        { originX: 'center', originY: 'bottom', overlayX: 'center', overlayY: 'top', offsetY: 6 },
        { originX: 'center', originY: 'top', overlayX: 'center', overlayY: 'bottom', offsetY: -6 },
      ])
      .withViewportMargin(8);
    this.#overlay ??= createOverlayRef(this.#injector, {
      positionStrategy: position,
      scrollStrategy: createRepositionScrollStrategy(this.#injector),
    });
    const bubble = this.#overlay.attach(new ComponentPortal(TooltipBubble));
    bubble.instance.id = this.#id;
    bubble.instance.text = this.appTooltip();
    bubble.changeDetectorRef.detectChanges();
    this.describedBy.set(this.#id);
  }

  hide(): void {
    clearTimeout(this.#timer);
    this.#overlay?.detach();
    this.describedBy.set(null);
  }

  ngOnDestroy(): void {
    clearTimeout(this.#timer);
    this.#overlay?.dispose();
  }
}
