import {
  Component,
  OnInit,
  computed,
  effect,
  inject,
  input,
  signal,
  untracked,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { ActivityEntry, CoreService, errorMessage } from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { Select, SelectOption } from '../shared/select';
import { ActivityList } from './activity-list';

/** The filter value for backups; any other value is an app's connection id. */
export const BACKUPS = 'backups';

/** Entries of one app (its connection id), of backups, or all (`''`). */
export function activityOf(entries: ActivityEntry[], filter: string): ActivityEntry[] {
  if (!filter) {
    return entries;
  }
  if (filter === BACKUPS) {
    return entries.filter((e) => e.type === 'backup');
  }
  return entries.filter((e) => e.type !== 'backup' && e.connectionId === filter);
}

/** What happened on this desktop. Stays here: not backed up. */
@Component({
  selector: 'app-activity-page',
  imports: [ActivityList, FormsModule, Select],
  host: { class: 'page' },
  template: `
    <div class="page-inner">
      <div class="page-head">
        <h1 class="page-title">Activities</h1>
        @if (!app() && entries().length) {
          <button type="button" class="action quiet" (click)="clear()">Clear activities</button>
        }
      </div>
      <p class="copy">
        Requests from apps and backups on this desktop. Activity stays here and is not backed up.
      </p>
      <div class="filter">
        <label class="field-label" for="activityFilter">Show</label>
        <app-select
          inputId="activityFilter"
          class="select"
          [options]="filterOptions()"
          [ngModel]="app() ?? ''"
          (ngModelChange)="show($event)"
        />
      </div>
      @if (error()) {
        <p class="sam-error" role="alert">{{ error() }}</p>
      }

      <app-activity-list [entries]="shown()" />
      @if (!shown().length) {
        <div class="sam-card empty-card">
          @if (app()) {
            <p class="title">Nothing here yet</p>
            <p class="copy">Pick "All activities" to see everything.</p>
          } @else {
            <p class="title">No activity yet</p>
            <p class="copy">Requests from apps and backups appear here.</p>
          }
        </div>
      }
    </div>
  `,
  styles: `
    .filter {
      display: flex;
      align-items: center;
      gap: 12px;
    }

    .select {
      min-width: 260px;
    }
  `,
})
export class ActivityPage implements OnInit {
  readonly #core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);
  readonly #router = inject(Router);
  /** `?app=`: an app's connection id, or "backups". */
  readonly app = input<string>();
  protected readonly entries = signal<ActivityEntry[]>([]);
  protected readonly error = signal('');
  protected readonly shown = computed(() => activityOf(this.entries(), this.app() ?? ''));
  protected readonly filterOptions = computed<SelectOption<string>[]>(() => [
    { value: '', label: 'All activities' },
    { value: BACKUPS, label: 'Backups' },
    ...(this.#core.bunkerState()?.connections ?? []).map((c) => ({
      value: c.id,
      label: `${c.name ?? 'App ' + c.clientPubkey.slice(0, 8) + '…'} · ${c.identityNick}`,
    })),
  ]);

  constructor() {
    effect(() => {
      if (this.#core.activityChanges() > 0) {
        untracked(() => void this.reload());
      }
    });
  }

  async ngOnInit(): Promise<void> {
    await Promise.all([this.reload(), this.#core.refreshBunker()]);
  }

  async reload(): Promise<void> {
    this.entries.set(await this.#core.activity().catch(() => []));
  }

  show(filter: string): void {
    void this.#router.navigate(['/activity'], { queryParams: filter ? { app: filter } : {} });
  }

  async clear(): Promise<void> {
    if (!(await this.#confirm.ask('Clear activities?', { yesLabel: 'Clear', danger: true }))) {
      return;
    }
    try {
      await this.#core.clearActivity();
      await this.reload();
    } catch (error) {
      this.error.set(errorMessage(error));
    }
  }
}
