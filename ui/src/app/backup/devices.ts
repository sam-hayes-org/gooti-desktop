import { Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CoreService, DeviceRow, errorMessage } from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';

const DAY = 24 * 60 * 60 * 1000;

/** "today", "yesterday", "5 days ago", by calendar day. Same words as the extension. */
export function daysAgo(at: number, now = Date.now()): string {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const days = Math.max(0, Math.ceil((today.getTime() - at) / DAY));
  return days === 0 ? 'today' : days === 1 ? 'yesterday' : `${days} days ago`;
}

/**
 * Removing only tidies the list. For a browser, the message names what does stop it: Disconnect
 * in Connections.
 */
export function removeMessage(device: Pick<DeviceRow, 'name' | 'app'>): string {
  const base = `Remove “${device.name}” from the list? It shows up again if it backs up.`;
  return device.app === 'browser'
    ? `${base} To stop a lost browser from signing through this desktop, disconnect it under Connections → Browsers.`
    : base;
}

/**
 * "Devices" on the Backup page: every Gooti that uses this backup, from their device records on
 * the relays. Removing one only takes it off the list; it comes back when it backs up again.
 */
@Component({
  selector: 'app-backup-devices',
  imports: [FormsModule],
  template: `
    <section class="group" aria-label="Devices">
      <p class="kicker">Devices</p>
      <p class="copy">
        Each Gooti that uses this backup. Remove one you no longer use from the list: it comes back if it
        backs up again.
      </p>
      @if (loading() && devices().length === 0) {
        <p class="copy">Reading the relays…</p>
      }
      @for (device of devices(); track device.id) {
        <div class="device" [class.here]="device.here">
          <i class="bi" [class.bi-window]="device.app === 'browser'" [class.bi-display]="device.app === 'desktop'"></i>
          @if (device.here && renaming()) {
            <form class="rename" (submit)="$event.preventDefault(); saveName()">
              <div class="sam-field grow">
                <input
                  type="text"
                  name="deviceName"
                  aria-label="Name of this desktop"
                  maxlength="80"
                  autocomplete="off"
                  [(ngModel)]="nameDraft"
                  [disabled]="busy()"
                />
              </div>
              <button type="submit" class="action primary" [disabled]="busy() || !nameDraft.trim()">Save</button>
              <button type="button" class="action quiet" [disabled]="busy()" (click)="renaming.set(false)">Cancel</button>
            </form>
          } @else {
            <span class="text">
              <span class="name">{{ device.name }}</span>
              <span class="note">{{ device.here ? 'This desktop · ' : '' }}{{ note(device) }}</span>
            </span>
            @if (device.here) {
              <button type="button" class="action quiet" [disabled]="busy()" (click)="startRename(device)">Rename</button>
            } @else {
              <button type="button" class="action quiet" [disabled]="busy()" (click)="remove(device)">Remove from list</button>
            }
          }
        </div>
      }
      @if (error()) {
        <p class="sam-error" role="alert">{{ error() }}</p>
        <button type="button" class="action quiet retry" [disabled]="loading()" (click)="load()">Try again</button>
      }
    </section>
  `,
  styles: `
    :host {
      display: block;
    }

    .group {
      margin-top: 10px;
    }

    .device {
      display: flex;
      align-items: center;
      gap: 12px;
      min-height: 56px;
      padding: 8px 12px 8px 14px;
      border-radius: 8px;
      background: var(--background-light);

      > .bi {
        flex: none;
        color: var(--muted);
        font-size: 20px;
      }

      &.here > .bi {
        color: #c5dbff;
      }
    }

    .text {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-width: 0;
    }

    .name {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: #ffffff;
      font-weight: 500;
    }

    .note {
      color: var(--muted);
      font-size: 13px;
    }

    .rename {
      display: flex;
      align-items: center;
      gap: 8px;
      flex: 1;
      min-width: 0;
    }

    .retry {
      align-self: flex-start;
    }
  `,
})
export class BackupDevices implements OnInit {
  readonly #core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);

  protected readonly devices = signal<DeviceRow[]>([]);
  protected readonly loading = signal(false);
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly renaming = signal(false);
  protected nameDraft = '';

  async ngOnInit(): Promise<void> {
    await this.load();
  }

  async load(): Promise<void> {
    this.loading.set(true);
    this.error.set('');
    try {
      this.devices.set(await this.#core.backupDevices());
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.loading.set(false);
    }
  }

  protected note(device: DeviceRow): string {
    return device.backedUpAt === null
      ? 'Shows up here after its next backup'
      : `Backed up ${daysAgo(device.backedUpAt)}`;
  }

  protected startRename(device: DeviceRow): void {
    this.nameDraft = device.name;
    this.renaming.set(true);
  }

  async saveName(): Promise<void> {
    await this.#run(async () => {
      this.devices.set(await this.#core.renameDevice(this.nameDraft));
      this.renaming.set(false);
    });
  }

  async remove(device: DeviceRow): Promise<void> {
    const yes = await this.#confirm.ask(removeMessage(device), { yesLabel: 'Remove' });
    if (!yes) {
      return;
    }
    await this.#run(async () => {
      this.devices.set(await this.#core.removeDevice(device.id));
    });
  }

  async #run(work: () => Promise<void>): Promise<void> {
    this.busy.set(true);
    this.error.set('');
    try {
      await work();
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
