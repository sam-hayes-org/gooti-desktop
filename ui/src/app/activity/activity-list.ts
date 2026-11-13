import { Component, inject, input } from '@angular/core';
import { ActivityEntry, CoreService } from '../core/core.service';
import { methodLabel } from '../connections/kinds';

/** "Approved by you", "Rejected by a remembered choice", "Nobody answered in time". */
function decision(approved: boolean, reason: string): string {
  const verb = approved ? 'Approved' : 'Rejected';
  switch (reason) {
    case 'remembered':
      return `${verb} by a remembered choice`;
    case 'trustLevel':
      return `${verb} by the app's trust level`;
    case 'timedOut':
      return 'Nobody answered in time';
    default:
      return `${verb} by you`;
  }
}

const BACKUP_RESULTS: Record<string, string> = {
  backedUp: 'Backed up',
  // Entries written before 2026-11-12.
  synced: 'Backed up',
  updated: 'Updated',
  current: 'Already up to date.',
  failed: 'Failed',
};

/** Activity entries, as on the Activities page and an app's page. */
@Component({
  selector: 'app-activity-list',
  template: `
    @for (entry of entries(); track $index) {
      <div class="entry" [attr.data-type]="entry.type">
        <i class="bi" [class]="icon(entry)"></i>
        <div class="text">
          <span class="title">{{ title(entry) }}</span>
          <span class="copy">{{ detail(entry) }}</span>
          @if (entry.type === 'backup') {
            @for (line of entry.took; track $index) {
              <span class="copy line">On this desktop: {{ line }}</span>
            }
            @for (line of entry.sent; track $index) {
              <span class="copy line">Sent: {{ line }}</span>
            }
          }
        </div>
        <span class="when">{{ when(entry.at) }}</span>
      </div>
    }
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: 14px;
    }

    /* Not .row, which is Bootstrap's grid class. */
    .entry {
      display: flex;
      align-items: flex-start;
      gap: 12px;
      padding: 10px 14px;
      border-radius: 10px;
      background: var(--background-light);
    }

    .bi {
      margin-top: 2px;
      font-size: 16px;
      color: var(--muted);
    }

    .bi-check-circle {
      color: #4cc38a;
    }

    .bi-x-circle,
    .bi-exclamation-circle {
      color: #f0a8a8;
    }

    .text {
      flex: 1;
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 2px;
    }

    .title {
      font-weight: 500;
    }

    .line {
      font-size: 13px;
    }

    .when {
      flex: none;
      color: var(--muted);
      font-size: 13px;
    }
  `,
})
export class ActivityList {
  readonly #core = inject(CoreService);
  readonly entries = input.required<ActivityEntry[]>();

  protected icon(entry: ActivityEntry): string {
    switch (entry.type) {
      case 'request':
        return entry.approved && !entry.error ? 'bi-check-circle' : 'bi-x-circle';
      case 'connected':
        return 'bi-plug';
      case 'disconnected':
        return 'bi-plug-fill';
      case 'backup':
        return entry.result === 'failed' ? 'bi-exclamation-circle' : 'bi-arrow-repeat';
    }
  }

  protected title(entry: ActivityEntry): string {
    switch (entry.type) {
      case 'request':
        return `${entry.site ? `${entry.site} through ${entry.app}` : entry.app}: ${methodLabel(entry.method, entry.kind)}`;
      case 'connected':
        return `${entry.app} connected`;
      case 'disconnected':
        return `${entry.app} disconnected`;
      case 'backup':
        return `${entry.automatic ? 'Automatic backup' : 'Back up now'}: ${BACKUP_RESULTS[entry.result] ?? entry.result}`;
    }
  }

  protected detail(entry: ActivityEntry): string {
    const nick = (pubkey: string) =>
      this.#core.identities().find((i) => i.pubkey === pubkey)?.nick ?? 'a deleted identity';
    switch (entry.type) {
      case 'request': {
        const failed = entry.error ? ` It failed: ${entry.error}` : '';
        return `${decision(entry.approved, entry.reason)}, as ${nick(entry.identityPubkey)}.${failed}`;
      }
      case 'connected':
      case 'disconnected':
        return `As ${nick(entry.identityPubkey)}`;
      case 'backup':
        return entry.error ?? '';
    }
  }

  protected when(at: number): string {
    return new Date(at).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  }
}
