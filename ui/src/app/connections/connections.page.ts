import { Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import {
  AgentInfo,
  AgentScope,
  BrowserList,
  BunkerAsk,
  CoreService,
  Trust,
  errorMessage,
} from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { CopyButton } from '../shared/copy-button';
import { Select, SelectOption } from '../shared/select';
import { Tooltip } from '../shared/tooltip';
import { TRUST_CHOICES } from './connection.page';
import { methodLabel } from './kinds';

export const TRUST_LABELS: Record<Trust, string> = {
  ask: 'Asks every time',
  basic: 'Basic',
  trust: 'Trusted',
};

/** The hosted Gooti MCP server (gooti-mcp). */
export const MCP_URL = 'https://mcp.getgooti.com';

@Component({
  selector: 'app-connections-page',
  imports: [FormsModule, RouterLink, Select, CopyButton, Tooltip],
  host: { class: 'page' },
  templateUrl: './connections.page.html',
  styleUrl: './connections.page.scss',
})
export class ConnectionsPage implements OnInit {
  protected readonly core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);

  /** Live: the core reloads it when an app or a relay changes. */
  protected readonly info = this.core.bunkerState;
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly notice = signal('');
  /** "Browsers": each browser with its own key. */
  protected readonly browsers = signal<BrowserList | null>(null);
  /** Where AI assistants add Gooti MCP. */
  protected readonly mcpUrl = MCP_URL;
  /** "AI agents": each agent with its scope and identities. */
  protected readonly agents = signal<AgentInfo[] | null>(null);
  /** Apps, without the browsers and the AI agents: those have their own sections. */
  protected readonly apps = computed(() =>
    (this.info()?.connections ?? []).filter((c) => !c.browsers && !c.agent),
  );
  constructor() {
    effect(() => {
      // A browser or agent that connected elsewhere, or a backup, changes the connections: read again.
      if (this.info()) {
        untracked(() => {
          void this.#loadBrowsers();
          void this.#loadAgents();
        });
      }
    });
  }

  async #loadBrowsers(): Promise<void> {
    this.browsers.set(await this.core.bunkerBrowsers().catch(() => null));
  }

  async #loadAgents(): Promise<void> {
    this.agents.set(await this.core.bunkerAgents().catch(() => null));
  }

  protected scopeOptions(agent: AgentInfo): SelectOption<AgentScope>[] {
    const home = agent.homeNick ?? 'its identity';
    return [
      { value: 'identity', label: `Only ${home}` },
      { value: 'desktopOnly', label: `${home} and all Desktop only identities` },
    ];
  }

  async setAgentScope(agent: AgentInfo, scope: AgentScope): Promise<void> {
    if (scope === agent.scope) {
      return;
    }
    if (scope === 'desktopOnly') {
      const yes = await this.#confirm.ask(
        `Let ${agent.name} use all your Desktop only identities, and create new ones? Gooti asks you before each new identity.`,
        { yesLabel: 'Widen scope' },
      );
      if (!yes) {
        // The select shows the old scope again.
        this.agents.update((list) => list?.map((a) => ({ ...a })) ?? null);
        return;
      }
    }
    await this.#run(async () => {
      this.agents.set(await this.core.bunkerAgentSetScope(agent.clientPubkey, scope));
    });
  }

  /** The agent being renamed, and the name typed so far. */
  protected readonly renaming = signal<string | null>(null);
  protected readonly newName = signal('');

  startRename(agent: AgentInfo): void {
    this.newName.set(agent.name);
    this.renaming.set(agent.clientPubkey);
  }

  async saveName(agent: AgentInfo): Promise<void> {
    const name = this.newName().trim();
    if (!name || name === agent.name) {
      this.renaming.set(null);
      return;
    }
    await this.#run(async () => {
      this.agents.set(await this.core.bunkerAgentRename(agent.clientPubkey, name));
      this.renaming.set(null);
    });
  }

  async disconnectAgent(agent: AgentInfo): Promise<void> {
    const yes = await this.#confirm.ask(
      `Disconnect ${agent.name}? It can no longer use any of your identities until you connect it again.`,
      { yesLabel: 'Disconnect', danger: true },
    );
    if (!yes) {
      return;
    }
    await this.#run(async () => {
      this.agents.set(await this.core.bunkerAgentDisconnect(agent.clientPubkey));
    });
  }

  async disconnectBrowser(name: string, clientPubkey: string): Promise<void> {
    const yes = await this.#confirm.ask(`Disconnect ${name}? It can no longer sign through this desktop.`, {
      yesLabel: 'Disconnect',
      danger: true,
    });
    if (!yes) {
      return;
    }
    await this.#run(async () => {
      this.browsers.set(await this.core.bunkerBrowserDisconnect(clientPubkey));
    });
  }

  async ngOnInit(): Promise<void> {
    // "Connect new app" comes back here with a note once the app connected.
    const notice = (history.state as { notice?: unknown } | null)?.notice;
    if (typeof notice === 'string') {
      this.notice.set(notice);
    }
    await this.reload();
  }

  async reload(): Promise<void> {
    await this.core.refreshBunker();
  }

  /** Every app is answered by another Gooti Desktop, so this one listens on nothing. */
  protected allElsewhere(): boolean {
    const connections = this.info()?.connections ?? [];
    return connections.length > 0 && connections.every((c) => c.servedElsewhere);
  }

  /** Who uses a relay: the apps on it, and a link waiting for an app. */
  protected usedBy(url: string): string {
    const info = this.info();
    if (!info) {
      return '';
    }
    const names = [...new Set(info.connections.filter((c) => c.relays.includes(url)).map((c) => this.appName(c)))];
    if (info.linkRelays.includes(url)) {
      names.push('a new link');
    }
    return names.join(', ');
  }

  async remove(id: string): Promise<void> {
    if (!(await this.#confirm.ask('Remove this app?', { yesLabel: 'Remove', danger: true }))) {
      return;
    }
    await this.#run(async () => {
      await this.core.bunkerRevoke(id);
      await this.reload();
    });
  }

  async setPaused(box: HTMLInputElement): Promise<void> {
    await this.#run(async () => {
      await this.core.bunkerSetPaused(box.checked);
      await this.reload();
    });
    box.checked = this.info()?.paused ?? false;
  }

  protected readonly trustLabel: Record<Trust, string> = TRUST_LABELS;

  /** The trust level of one of an agent's connections (one per identity). */
  protected trustOf(connectionId: string): Trust | null {
    return this.info()?.connections.find((c) => c.id === connectionId)?.trust ?? null;
  }

  /** Its trust level and what it means, on hover, so the identity need not be opened to see it. */
  protected trustHint(connectionId: string): string {
    const choice = TRUST_CHOICES.find((c) => c.value === this.trustOf(connectionId));
    return choice ? `${choice.label}: ${choice.copy}` : 'Open to set how far it may go without asking.';
  }

  protected askLabel(ask: BunkerAsk): string {
    if (ask.method === 'connect') {
      return `${ask.details.browser?.name ?? 'A browser'} wants to sign through this desktop`;
    }
    if (ask.method === 'gooti_create_identity') {
      const agent = this.agents()?.find((a) => a.clientPubkey === ask.clientPubkey);
      return `${agent?.name ?? 'An AI agent'} wants to create the identity ${ask.details.newIdentity?.nick ?? ''}`;
    }
    const app = this.info()?.connections.find((c) => c.id === ask.connectionId);
    return `${app ? this.appName(app) : 'An app'}: ${methodLabel(ask.method, ask.details.kind)}`;
  }

  async decide(key: string, approve: boolean): Promise<void> {
    await this.#run(() => this.core.bunkerDecide(key, approve));
  }

  protected appName(connection: { name: string | null; clientPubkey: string }): string {
    return connection.name ?? `App ${connection.clientPubkey.slice(0, 8)}…`;
  }

  protected when(ms: number | null): string {
    return ms ? new Date(ms).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) : 'Never';
  }

  async #run(work: () => Promise<void>): Promise<void> {
    if (this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    this.notice.set('');
    try {
      await work();
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
