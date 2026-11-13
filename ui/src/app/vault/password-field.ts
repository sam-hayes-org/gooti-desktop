import { Component, ElementRef, input, model, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';

/** Password input with the eye toggle from the extension's vault screens. */
@Component({
  selector: 'app-password-field',
  imports: [FormsModule],
  template: `
    <div class="field">
      <input
        #input
        type="password"
        [placeholder]="placeholder()"
        [attr.aria-label]="placeholder()"
        [(ngModel)]="value"
        [ngModelOptions]="{ standalone: true }"
        [name]="name()"
        [attr.autofocus]="autofocus() ? '' : null"
        autocomplete="off"
      />
      <button
        type="button"
        class="eye"
        [attr.aria-label]="input.type === 'password' ? 'Show password' : 'Hide password'"
        (click)="toggle()"
      >
        <i
          class="bi"
          [class.bi-eye]="input.type === 'password'"
          [class.bi-eye-slash]="input.type === 'text'"
        ></i>
      </button>
    </div>
  `,
  styleUrl: './vault-screen.scss',
  styles: `
    :host {
      height: auto;
    }
  `,
})
export class PasswordField {
  readonly value = model('');
  readonly placeholder = input('Gooti password');
  readonly name = input('password');
  readonly autofocus = input(false);
  private readonly inputElement = viewChild.required<ElementRef<HTMLInputElement>>('input');

  toggle(): void {
    const element = this.inputElement().nativeElement;
    element.type = element.type === 'password' ? 'text' : 'password';
  }

  focus(): void {
    this.inputElement().nativeElement.focus();
  }
}
