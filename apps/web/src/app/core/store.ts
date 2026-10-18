import { HttpClient, HttpErrorResponse, httpResource } from '@angular/common/http';
import { DestroyRef, Injectable, computed, inject, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import type {
  Attempt,
  AttemptAction,
  Catalog,
  CreateSession,
  PracticeSelection,
  PracticeSession,
  Progress,
  SessionNextRequest,
  Settings,
} from '@math/contract';

export function messageOf(error: unknown): string {
  if (error instanceof HttpErrorResponse)
    return typeof error.error?.error === 'string'
      ? error.error.error
      : error.status === 0
        ? 'Cannot reach the server. This app needs an internet connection.'
        : `Request failed (${error.status}). Please try again.`;
  return error instanceof Error ? error.message : 'Something went wrong. Please try again.';
}

interface SavedSession {
  id: string;
  creation?: CreateSession;
  next?: SessionNextRequest;
}
const sessionKey = 'math.practice-session';

@Injectable({ providedIn: 'root' })
export class Store {
  readonly http = inject(HttpClient);
  readonly catalog = httpResource<Catalog>(() => '/api/catalog');
  readonly settings = httpResource<Settings>(() => '/api/settings');
  readonly progress = httpResource<Progress>(() => '/api/progress');
  readonly units = computed(() => (this.catalog.hasValue() ? this.catalog.value().units : []));
  readonly topics = computed(() =>
    this.units().flatMap((unit) =>
      unit.topics.map((topic) => ({
        ...topic,
        unitId: unit.id,
        label: `${unit.module} · ${unit.number} · ${topic.title}`,
      })),
    ),
  );
  readonly saving = signal(false);
  readonly settingsError = signal('');
  private savedSession = this.restoreSession();
  readonly sessionId = signal<string | null>(this.savedSession?.id ?? null);
  readonly session = signal<PracticeSession | null>(null);
  readonly current = signal<Attempt | null>(null);
  readonly navigationPending = signal(!!this.savedSession?.next);
  readonly busy = signal(false);
  readonly refreshing = signal(false);
  readonly actionError = signal('');
  readonly resumeError = signal('');
  readonly completed = computed(() => {
    const session = this.session();
    if (!session) return 0;
    const current = this.current();
    const lastId = session.attemptIds.at(-1);
    return (
      session.attemptIds.length -
      (lastId && (current?.id !== lastId || current.outcome === 'unfinished') ? 1 : 0)
    );
  });
  private revision = 0;
  practiceActive = false;

  constructor() {
    if (this.sessionId()) void this.refreshSession();
    const timer = setInterval(() => {
      if (this.session()?.status === 'generating' && !this.resumeError())
        void this.refreshSession();
    }, 2000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  topicName(id: string): string {
    return this.topics().find((topic) => topic.id === id)?.title ?? id;
  }
  async saveSettings(patch: Partial<Settings>): Promise<boolean> {
    if (this.saving() || !this.settings.hasValue() || this.settings.isLoading()) return false;
    this.saving.set(true);
    this.settingsError.set('');
    try {
      const saved = await firstValueFrom(
        this.http.put<Settings>('/api/settings', { ...this.settings.value(), ...patch }),
      );
      this.settings.value.set(saved);
      this.progress.reload();
      return true;
    } catch (error) {
      this.settingsError.set(messageOf(error));
      return false;
    } finally {
      this.saving.set(false);
    }
  }
  async start(request: PracticeSelection, length: number): Promise<boolean> {
    if (this.busy() || this.saving() || this.sessionId()) return false;
    const creation: CreateSession = { ...request, requestId: crypto.randomUUID(), length };
    // Save the idempotency key before sending, including across a lost response or reload.
    this.persist({ id: creation.requestId, creation });
    this.current.set(null);
    return this.createSession(creation);
  }
  private async createSession(creation: CreateSession): Promise<boolean> {
    this.busy.set(true);
    ++this.revision;
    this.actionError.set('');
    this.resumeError.set('');
    let created = false;
    try {
      const session = await firstValueFrom(
        this.http.post<PracticeSession>('/api/sessions', creation),
      );
      this.session.set(session);
      this.persist({ id: session.id });
      created = true;
    } catch (error) {
      // A definitive rejection did not create a session. An uncertain response must replay the same key.
      if (error instanceof HttpErrorResponse && error.status >= 400 && error.status < 500) {
        this.persist(null);
        this.actionError.set(messageOf(error));
      } else {
        this.resumeError.set(messageOf(error));
      }
    } finally {
      this.busy.set(false);
    }
    if (created) await this.refreshSession();
    return created;
  }
  async refreshSession(): Promise<void> {
    if (!this.savedSession || this.busy() || this.refreshing()) return;
    if (this.savedSession.creation) {
      await this.createSession(this.savedSession.creation);
      return;
    }
    const id = this.savedSession.id;
    const revision = this.revision;
    this.refreshing.set(true);
    this.resumeError.set('');
    let presentFirst = false;
    let replayNext = false;
    try {
      const session = await firstValueFrom(
        this.http.get<PracticeSession>(`/api/sessions/${encodeURIComponent(id)}`),
      );
      if (revision !== this.revision || id !== this.sessionId()) return;
      this.session.set(session);
      replayNext = !!this.savedSession?.next && session.status === 'ready';
      const attemptId = session.attemptIds.at(-1);
      if (attemptId && this.current()?.id !== attemptId && !replayNext) {
        const attempt = await firstValueFrom(
          this.http.get<Attempt>(`/api/attempts/${encodeURIComponent(attemptId)}`),
        );
        // Never let an older read erase an action response, even if it arrives last.
        if (revision !== this.revision || id !== this.sessionId()) return;
        this.current.set(attempt);
      }
      presentFirst = session.status === 'ready' && !attemptId;
    } catch (error) {
      if (revision === this.revision) this.resumeError.set(messageOf(error));
    } finally {
      this.refreshing.set(false);
    }
    if (this.practiceActive && (replayNext || presentFirst)) await this.next();
  }
  async next(): Promise<void> {
    const session = this.session();
    const current = this.current();
    if (!session || session.status !== 'ready' || this.busy() || this.refreshing()) return;
    const pending = this.savedSession?.next;
    if (!pending && session.attemptIds.length && (!current || current.outcome === 'unfinished'))
      return;
    if (!pending && session.attemptIds.length >= session.length) return;
    const request: SessionNextRequest = pending ?? {
      eventId: crypto.randomUUID(),
      afterAttemptId: current?.id ?? null,
    };
    this.persist({ id: session.id, next: request });
    this.busy.set(true);
    ++this.revision;
    this.actionError.set('');
    let advanced = false;
    try {
      const attempt = await firstValueFrom(
        this.http.post<Attempt>(`/api/sessions/${encodeURIComponent(session.id)}/next`, request),
      );
      this.current.set(attempt);
      this.session.set({
        ...session,
        attemptIds: session.attemptIds.includes(attempt.id)
          ? session.attemptIds
          : [...session.attemptIds, attempt.id],
      });
      this.persist({ id: session.id });
      this.progress.reload();
      advanced = true;
    } catch (error) {
      this.actionError.set(messageOf(error));
    } finally {
      this.busy.set(false);
    }
    if (advanced) await this.refreshSession();
  }
  async act(action: Omit<AttemptAction, 'eventId'>): Promise<void> {
    const current = this.current();
    if (!current || this.busy() || this.savedSession?.next) return;
    this.busy.set(true);
    ++this.revision;
    this.actionError.set('');
    try {
      const attempt = await firstValueFrom(
        this.http.post<Attempt>(`/api/attempts/${encodeURIComponent(current.id)}/actions`, {
          ...action,
          eventId: crypto.randomUUID(),
        }),
      );
      this.current.set(attempt);
      this.progress.reload();
    } catch (error) {
      this.actionError.set(messageOf(error));
    } finally {
      this.busy.set(false);
    }
  }
  async finish(): Promise<boolean> {
    const id = this.sessionId();
    if (!id || this.busy() || this.refreshing()) return false;
    this.busy.set(true);
    ++this.revision;
    this.actionError.set('');
    try {
      await firstValueFrom(
        this.http.post<PracticeSession>(`/api/sessions/${encodeURIComponent(id)}/finish`, null),
      );
      this.persist(null);
      this.session.set(null);
      this.current.set(null);
      this.resumeError.set('');
      this.progress.reload();
      return true;
    } catch (error) {
      this.actionError.set(messageOf(error));
      return false;
    } finally {
      this.busy.set(false);
    }
  }
  private persist(saved: SavedSession | null): void {
    this.savedSession = saved;
    this.sessionId.set(saved?.id ?? null);
    this.navigationPending.set(!!saved?.next);
    try {
      if (saved) localStorage.setItem(sessionKey, JSON.stringify(saved));
      else localStorage.removeItem(sessionKey);
    } catch {
      /* This tab can still resume the server session when browser storage is unavailable. */
    }
  }
  private restoreSession(): SavedSession | null {
    try {
      localStorage.removeItem('math.session');
      const saved: unknown = JSON.parse(localStorage.getItem(sessionKey) ?? 'null');
      if (saved && typeof saved === 'object' && 'id' in saved && typeof saved.id === 'string')
        return saved as SavedSession;
    } catch {
      /* Invalid or unavailable local state never prevents loading server history. */
    }
    return null;
  }
}
