import { Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { AgentScope, CoreService, errorMessage, isAgentLink } from '../core/core.service';
import { Select, SelectOption } from '../shared/select';

export const AGENT_CONNECTED_NOTICE = 'The AI assistant is connected. It is listed under AI agents.';

/** What a Gooti MCP `nostrconnect://` link says about the agent. */
export function agentOfLink(link: string): { name: string; host: string | null } {
  const query = new URLSearchParams(link.trim().split('?')[1] ?? '');
  let host: string | null = null;
  try {
    host = new URL(query.get('url') ?? '').host || null;
  } catch {
    host = null;
  }
  return { name: query.get('name')?.trim() || 'An AI assistant', host };
}

/**
 * "Connect an AI assistant": a `nostrconnect://` link from Gooti MCP (`gooti=agent`). Who asks, its
 * default identity, and what it may use. Ordinary apps use "Connect new app".
 */
@Component({
  selector: 'app-agent-connect-page',
  imports: [FormsModule, Select],
  host: { class: 'page' },
  templateUrl: './agent-connect.page.html',
  styleUrl: './agent-connect.page.scss',
})
export class AgentConnectPage {
  protected readonly core = inject(CoreService);
  readonly #router = inject(Router);

  protected readonly link = signal('');
  protected readonly identityId = signal('');
  protected readonly scope = signal<AgentScope>('identity');
  protected readonly busy = signal(false);
  protected readonly error = signal('');

  protected readonly agent = computed(() => agentOfLink(this.link()));
  /** Identities this desktop can sign for. */
  protected readonly signable = computed(() => this.core.identities().filter((i) => i.hasPrivateKey));
  protected readonly identityOptions = computed<SelectOption<string>[]>(() =>
    this.signable().map((i) => ({ value: i.id, label: i.nick })),
  );
  protected readonly homeNick = computed(
    () => this.signable().find((i) => i.id === this.identityId())?.nick ?? 'this identity',
  );
  /** What the wider scope covers today, besides the default identity. */
  protected readonly desktopOnly = computed(() =>
    this.signable()
      .filter((i) => i.custody === 'desktop' && i.id !== this.identityId())
      .map((i) => i.nick),
  );

  constructor() {
    // A link pasted into "Connect new app" comes along in the navigation state.
    const state = this.#router.getCurrentNavigation()?.extras.state ?? (history.state as unknown);
    const passed = (state as { link?: unknown } | null)?.link;
    if (typeof passed === 'string' && isAgentLink(passed)) {
      this.link.set(passed.trim());
    }
    effect(() => {
      // A link clicked in the browser's sign-in page. Connecting stays the user's step.
      const clicked = this.core.connectLink();
      if (clicked && isAgentLink(clicked)) {
        untracked(() => {
          this.core.connectLink.set(null);
          this.link.set(clicked.trim());
          this.scope.set('identity');
        });
      }
    });
    effect(() => {
      const first = this.signable()[0];
      if (first && !untracked(this.identityId)) {
        this.identityId.set(first.id);
      }
    });
  }

  async connect(): Promise<void> {
    if (this.busy() || !this.link()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    try {
      await this.core.bunkerConnect(this.link(), this.identityId(), this.scope());
      void this.#router.navigateByUrl('/connections', { state: { notice: AGENT_CONNECTED_NOTICE } });
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }

  back(): void {
    void this.#router.navigateByUrl('/connections');
  }
}
