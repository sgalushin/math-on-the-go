import { Component, DestroyRef, computed, effect, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { FormsModule } from '@angular/forms';
import type { PracticeSelection } from '@math/contract';
import { Store } from '../core/store';
import { AttemptView } from '../shared/attempt-view';

@Component({
  standalone: true,
  imports: [RouterLink, FormsModule, AttemptView],
  template: `
    @if (store.sessionId()) {
      <header class="session-heading">
        <div>
          <p class="eyebrow">Your saved session</p>
          <h1>
            {{
              store.session()?.request?.mode === 'review'
                ? 'Review flashcards'
                : store.session()?.request?.kind === 'formula'
                  ? 'Formula flashcards'
                  : 'Practice'
            }}
          </h1>
        </div>
        <a routerLink="/" class="text-link">Pause & leave</a>
      </header>
      @if (store.resumeError()) {
        <div class="error" role="alert">
          <p>{{ store.resumeError() }}</p>
          <button
            class="button secondary"
            (click)="store.refreshSession()"
            [disabled]="store.busy() || store.refreshing()"
          >
            Retry resume
          </button>
          <p class="small">
            Your saved session has not been replaced. Retrying will resume the same request.
          </p>
        </div>
      }
      @if (store.session(); as session) {
        @if (session.status === 'finished' && session.error) {
          <section class="panel" role="status">
            <h2>Switch to the handbook deck</h2>
            <p>{{ session.error }}</p>
            <p>Your previous attempts remain in history.</p>
            <button
              class="button"
              (click)="finish()"
              [disabled]="store.busy() || store.refreshing()"
            >
              Start a new session
            </button>
          </section>
        } @else if (session.status === 'generating') {
          <section class="panel preparation" aria-live="polite" aria-busy="true">
            <p class="eyebrow">Getting everything ready</p>
            <h2>Preparing your tasks</h2>
            <p>
              Generated tasks are independently checked. Formula cards come from the fixed handbook
              deck and are never generated.
            </p>
            <div class="session-meter">
              <strong>Ready {{ session.generated }} / {{ session.length }}</strong>
              <progress
                [value]="session.generated"
                [max]="session.length"
                aria-label="Prepared questions"
              ></progress>
            </div>
            <p class="small muted">
              You can leave this page and return. Preparation continues, and this session will
              resume here.
            </p>
          </section>
        } @else if (session.status === 'failed') {
          <section class="panel">
            <p class="eyebrow">Session not started</p>
            <h2>We couldn't prepare the full session.</h2>
            <p>{{ session.error || 'Some questions did not pass verification.' }}</p>
            <p>
              Verified {{ session.generated }} / {{ session.length }}. No partial session or
              replacement questions will be served.
            </p>
            <button
              class="button"
              (click)="finish()"
              [disabled]="store.busy() || store.refreshing()"
            >
              Start over
            </button>
          </section>
        } @else {
          <div class="session-meter">
            <span>{{ store.completed() }} / {{ session.length }} completed</span>
            <progress
              [value]="store.completed()"
              [max]="session.length"
              aria-label="Session completion"
            ></progress>
          </div>
          @if (session.request.mode === 'review') {
            <p class="small muted">
              This finite review queue contains {{ session.length }} available due or new cards.
            </p>
          } @else if (session.request.kind === 'formula') {
            <p class="small muted">
              {{ session.length }} fixed handbook cards. Due and new cards come first, followed by
              extra recall practice.
            </p>
          }
          @if (store.current(); as attempt) {
            <p class="topic-label">{{ store.topicName(attempt.question.topicId) }}</p>
            <attempt-view
              [attempt]="attempt"
              [interactive]="session.status === 'ready'"
              [busy]="store.busy() || store.navigationPending()"
              (action)="store.act($event)"
            />
            <div class="session-actions">
              @if (session.status === 'ready' && attempt.outcome === 'unfinished') {
                <button
                  class="text-button"
                  (click)="store.act({ action: 'skip' })"
                  [disabled]="store.busy() || store.navigationPending()"
                >
                  Skip this question
                </button>
              } @else {
                @if (session.status === 'ready' && store.completed() < session.length) {
                  <button
                    class="button"
                    (click)="store.next()"
                    [disabled]="store.busy() || store.refreshing()"
                  >
                    Next question <span aria-hidden="true">→</span>
                  </button>
                } @else {
                  <section class="session-complete">
                    <p class="eyebrow">A good place to pause</p>
                    <h2>
                      {{
                        store.completed() >= session.length
                          ? 'Session complete.'
                          : 'Session finished.'
                      }}
                    </h2>
                    <p>
                      {{ store.completed() }} questions recorded. Outcomes, hints and reasoning are
                      saved in your history.
                    </p>
                  </section>
                }
                @if (attempt.question.kind === 'task') {
                  <button
                    class="button secondary"
                    (click)="newTopicSession()"
                    [disabled]="store.busy() || store.refreshing() || store.navigationPending()"
                  >
                    New session on this topic
                  </button>
                  <p class="small muted">
                    Opens setup for a new full session. Questions are prepared only when you begin.
                  </p>
                }
              }
              <button
                class="text-button"
                (click)="finish()"
                [disabled]="store.busy() || store.refreshing()"
              >
                Finish session
              </button>
            </div>
          } @else if (session.status === 'ready') {
            <p class="status" role="status">Opening your prepared question…</p>
            @if (store.actionError()) {
              <button
                class="button"
                (click)="store.next()"
                [disabled]="store.busy() || store.refreshing()"
              >
                Retry opening question
              </button>
            }
          } @else {
            <p>Session finished. Your attempts remain in history.</p>
            <button class="button" (click)="finish()" [disabled]="store.busy()">
              Back to setup
            </button>
          }
        }
      } @else if (!store.resumeError()) {
        <p class="status" role="status">Resuming your session…</p>
      }
      @if (store.actionError()) {
        <p class="error" role="alert">{{ store.actionError() }}</p>
        @if (store.navigationPending()) {
          <button
            class="button"
            (click)="store.next()"
            [disabled]="store.busy() || store.refreshing()"
          >
            Retry next question
          </button>
        }
      }
      @if (store.busy()) {
        <p role="status" class="saving-status">Saving your session…</p>
      }
    } @else {
      <header class="page-heading">
        <p class="eyebrow">Your next session</p>
        <h1>
          {{
            mode === 'review'
              ? 'Review flashcards'
              : kind === 'formula'
                ? 'Formula flashcards'
                : 'Practice'
          }}
        </h1>
        <p class="lede">
          Start with what you have studied. Pick a smaller scope only if you want one.
        </p>
      </header>
      @if (finished()) {
        <p class="small" role="status">
          Session saved. <a routerLink="/history">View history →</a>
        </p>
      }
      @if (store.settings.hasValue()) {
        @if (!studiedUnits().length) {
          <section class="empty">
            <h2>What have you studied?</h2>
            <p>
              Mark the units you have covered in your profile. Your sessions can then use everything
              you have studied, or a smaller selection.
            </p>
            <a routerLink="/profile/units" class="button">Add studied units →</a>
          </section>
        } @else {
          <form class="panel setup-form" (ngSubmit)="begin()">
            <fieldset [disabled]="store.busy() || store.saving()">
              <legend>What would you like to practise?</legend>
              @if (mode !== 'review') {
                <div class="kind-options" role="group" aria-label="Practice type">
                  <button
                    type="button"
                    [class.chosen]="kind === 'task'"
                    [attr.aria-pressed]="kind === 'task'"
                    (click)="kind = 'task'"
                  >
                    Tasks
                  </button>
                  <button
                    type="button"
                    [class.chosen]="kind === 'formula'"
                    [attr.aria-pressed]="kind === 'formula'"
                    (click)="kind = 'formula'"
                  >
                    Formula flashcards
                  </button>
                  <button
                    type="button"
                    [class.chosen]="kind === 'mixed'"
                    [attr.aria-pressed]="kind === 'mixed'"
                    (click)="kind = 'mixed'"
                  >
                    Mixed
                  </button>
                </div>
                <p class="field-help kind-help">
                  {{
                    kind === 'formula'
                      ? 'Fixed formulas, identities and rules from your handbooks. Opens immediately — no AI generation.'
                      : kind === 'mixed'
                        ? 'Available handbook formula cards alongside newly generated tasks. Only tasks use AI.'
                        : 'Work through mathematical tasks with hints and explanations.'
                  }}
                </p>
              } @else {
                <p class="field-help">
                  Scheduled review of due and new handbook cards. No AI generation.
                </p>
              }
              <div class="scope-options">
                <label class="scope-choice" [class.chosen]="scope() === 'all'">
                  <input
                    type="radio"
                    name="scope"
                    value="all"
                    [checked]="scope() === 'all'"
                    (change)="setScope('all')"
                  />
                  <span
                    ><strong>Everything you've studied</strong
                    ><small
                      >{{ studiedUnits().length }} studied units · always up to date</small
                    ></span
                  >
                </label>
                <label class="scope-choice" [class.chosen]="scope() === 'specific'">
                  <input
                    type="radio"
                    name="scope"
                    value="specific"
                    [checked]="scope() === 'specific'"
                    (change)="setScope('specific')"
                  />
                  <span
                    ><strong>Choose specific units</strong
                    ><small>A selection for this session only</small></span
                  >
                </label>
              </div>
              @if (scope() === 'specific') {
                <div class="scope-units">
                  @for (group of unitGroups(); track group.module) {
                    <fieldset class="scope-module">
                      <legend>{{ group.module }}</legend>
                      @for (unit of group.units; track unit.id) {
                        <label class="check-label" [class.blocked]="!unitAvailable(unit.id)">
                          <input
                            type="checkbox"
                            [checked]="chosenUnits().includes(unit.id)"
                            [disabled]="!unitAvailable(unit.id)"
                            (change)="toggleUnit(unit.id)"
                          />
                          <span
                            ><strong>Unit {{ unit.number }} · {{ unit.title }}</strong>
                            <small>{{ unitAvailability(unit.id) }}</small></span
                          >
                        </label>
                      }
                    </fieldset>
                  }
                  @if (scopeUnits().length) {
                    <details class="topic-filter">
                      <summary>
                        Narrow by topic
                        <span>{{
                          chosenTopics().length
                            ? chosenTopics().length + ' selected'
                            : 'All eligible topics'
                        }}</span>
                      </summary>
                      <p class="small muted">
                        Optional. Leave every topic unchecked to use all eligible topics in your
                        chosen units.
                      </p>
                      @for (unit of scopeUnits(); track unit.id) {
                        <h3>{{ unit.module }} · Unit {{ unit.number }}</h3>
                        @for (topic of unit.topics; track topic.id) {
                          <label
                            class="topic-check"
                            [class.blocked]="missingPrerequisites(topic.prerequisites).length > 0"
                          >
                            <input
                              type="checkbox"
                              [checked]="chosenTopics().includes(topic.id)"
                              [disabled]="missingPrerequisites(topic.prerequisites).length > 0"
                              (change)="toggleTopic(topic.id)"
                            />
                            <span
                              >{{ topic.title
                              }}<small>{{ prerequisiteText(topic.prerequisites) }}</small></span
                            >
                          </label>
                        }
                      }
                    </details>
                  }
                </div>
              }
              <div class="pool-summary" aria-live="polite">
                <strong
                  >{{ scopeUnits().length }} {{ scope() === 'all' ? 'studied' : 'chosen' }} units ·
                  {{ eligibleTopics().length }} eligible topics</strong
                >
                @if (!scopeUnits().length) {
                  <p><strong>Choose at least one studied unit to begin.</strong></p>
                } @else if (!eligibleTopics().length) {
                  <p>
                    <strong>No eligible topics in this scope.</strong> Check the prerequisites below
                    or choose different units.
                  </p>
                }
                @if (blockedTopics().length) {
                  <details class="prerequisite-list">
                    <summary>{{ blockedTopics().length }} topics need prerequisites</summary>
                    @for (topic of blockedTopics(); track topic.id) {
                      <p class="small">
                        <strong>{{ topic.title }}</strong> —
                        {{ prerequisiteText(topic.prerequisites) }}
                      </p>
                    }
                  </details>
                }
                @if (blockedTopics().length || !eligibleTopics().length) {
                  <a routerLink="/profile/units" class="text-link">Manage studied history →</a>
                }
              </div>
              <button
                class="button full"
                type="submit"
                [disabled]="!eligibleTopics().length || store.busy() || store.saving()"
              >
                {{
                  store.busy()
                    ? mode === 'review' || kind === 'formula'
                      ? 'Opening your cards…'
                      : 'Preparing your tasks…'
                    : mode === 'review'
                      ? 'Begin review'
                      : kind === 'formula'
                        ? 'Begin flashcards'
                        : 'Begin practice'
                }}
                <span aria-hidden="true">→</span>
              </button>
              <details class="session-options">
                <summary>
                  Session options
                  <small
                    >{{ length }} questions ·
                    {{
                      mode === 'review'
                        ? 'flashcards'
                        : kind === 'mixed'
                          ? 'mixed'
                          : kind === 'task'
                            ? 'tasks'
                            : 'formula flashcards'
                    }}{{ mode !== 'review' && kind !== 'formula' ? ' · ' + difficulty : '' }}</small
                  >
                </summary>
                @if (mode !== 'review' && kind !== 'formula') {
                  <label for="practice-mode">Session style</label>
                  <select id="practice-mode" name="mode" [(ngModel)]="mode">
                    <option value="suggested">Suggested · weak & underpractised topics</option>
                    <option value="focused">Focused · my chosen scope</option>
                  </select>
                }
                @if (mode === 'review' || kind === 'formula') {
                  <p class="field-help">
                    Uses distinct handbook cards, up to your chosen length. Task difficulty does not
                    apply. Scheduled review uses due/new cards; formula practice also allows extra
                    recall before a card is due.
                  </p>
                } @else {
                  <label for="practice-difficulty">Difficulty</label>
                  <select id="practice-difficulty" name="difficulty" [(ngModel)]="difficulty">
                    <option value="any">All difficulties</option>
                    <option value="quick">Quick</option>
                    <option value="standard">Standard</option>
                    <option value="stretch">Stretch</option>
                  </select>
                }
                <label for="session-length">Session length</label>
                <div class="range-field">
                  <input
                    id="session-length"
                    name="length"
                    type="range"
                    min="1"
                    max="20"
                    step="1"
                    [(ngModel)]="length"
                  />
                  <output for="session-length">{{ length }} questions</output>
                </div>
                <p class="field-help">
                  Pause at any point. Your answers and unfinished attempts stay in history.
                </p>
              </details>
            </fieldset>
          </form>
        }
      }
      @if (store.actionError()) {
        <div class="error" role="alert">
          <p>{{ store.actionError() }}</p>
          <p class="small">
            No session was started. Check the message above or try a different eligible scope.
            Scheduled review needs available due or new cards.
          </p>
        </div>
      }
    }
  `,
})
export class PracticePage {
  readonly store = inject(Store);
  private readonly route = inject(ActivatedRoute);
  mode: PracticeSelection['mode'] = 'suggested';
  kind: PracticeSelection['kind'] = 'task';
  difficulty: PracticeSelection['difficulty'] = 'any';
  length = 5;
  readonly finished = signal(false);
  readonly scope = signal<'all' | 'specific'>('all');
  readonly chosenUnits = signal<string[]>([]);
  readonly chosenTopics = signal<string[]>([]);
  readonly studiedIds = computed(() =>
    this.store.settings.hasValue() ? this.store.settings.value().studiedUnitIds : [],
  );
  readonly studiedUnits = computed(() =>
    this.store.units().filter((unit) => this.studiedIds().includes(unit.id)),
  );
  readonly unitGroups = computed(() =>
    ['MST124', 'MST125']
      .map((module) => ({
        module,
        units: this.studiedUnits().filter((unit) => unit.module === module),
      }))
      .filter((group) => group.units.length),
  );
  readonly scopeUnits = computed(() =>
    this.scope() === 'all'
      ? this.studiedUnits()
      : this.studiedUnits().filter((unit) => this.chosenUnits().includes(unit.id)),
  );
  readonly scopeTopics = computed(() => this.scopeUnits().flatMap((unit) => unit.topics));
  readonly eligibleTopics = computed(() =>
    this.scopeTopics().filter(
      (topic) =>
        !this.missingPrerequisites(topic.prerequisites).length &&
        (this.scope() === 'all' ||
          !this.chosenTopics().length ||
          this.chosenTopics().includes(topic.id)),
    ),
  );
  readonly blockedTopics = computed(() =>
    this.scopeTopics().filter((topic) => this.missingPrerequisites(topic.prerequisites).length),
  );
  private initialized = false;
  constructor() {
    this.store.practiceActive = true;
    void this.store.refreshSession();
    inject(DestroyRef).onDestroy(() => {
      this.store.practiceActive = false;
    });
    const requested = this.route.snapshot.queryParamMap.get('mode');
    if (requested === 'review' || requested === 'focused' || requested === 'suggested')
      this.mode = requested;
    const kind = this.route.snapshot.queryParamMap.get('kind');
    if (kind === 'task' || kind === 'formula' || kind === 'mixed') this.kind = kind;
    effect(() => {
      if (this.store.settings.hasValue() && !this.initialized) {
        const settings = this.store.settings.value();
        this.length = settings.sessionLength;
        this.difficulty = settings.defaultDifficulty;
        this.initialized = true;
      }
    });
    effect(() => {
      const availableIds = this.studiedUnits()
        .filter((unit) => this.unitAvailable(unit.id))
        .map((unit) => unit.id);
      const units = this.chosenUnits().filter((id) => availableIds.includes(id));
      if (units.length !== this.chosenUnits().length) this.chosenUnits.set(units);
      const topics = this.store
        .topics()
        .filter(
          (topic) =>
            units.includes(topic.unitId) && !this.missingPrerequisites(topic.prerequisites).length,
        )
        .map((topic) => topic.id);
      const selected = this.chosenTopics().filter((id) => topics.includes(id));
      if (selected.length !== this.chosenTopics().length) this.chosenTopics.set(selected);
    });
  }
  setScope(scope: 'all' | 'specific'): void {
    this.scope.set(scope);
    if (scope === 'all') {
      this.chosenUnits.set([]);
      this.chosenTopics.set([]);
    }
  }
  missingPrerequisites(ids: string[]): string[] {
    return ids.filter((id) => !this.studiedIds().includes(id));
  }
  prerequisiteText(ids: string[]): string {
    const missing = this.missingPrerequisites(ids);
    return missing.length
      ? 'Needs studied: ' +
          missing
            .map((id) => {
              const unit = this.store.units().find((value) => value.id === id);
              return unit ? `${unit.module} Unit ${unit.number}` : id;
            })
            .join(', ')
      : 'All prerequisites covered';
  }
  unitAvailable(id: string): boolean {
    return !!this.studiedUnits()
      .find((unit) => unit.id === id)
      ?.topics.some((topic) => !this.missingPrerequisites(topic.prerequisites).length);
  }
  unitAvailability(id: string): string {
    const topics = this.studiedUnits().find((unit) => unit.id === id)?.topics ?? [];
    const eligible = topics.filter(
      (topic) => !this.missingPrerequisites(topic.prerequisites).length,
    ).length;
    const missing = [
      ...new Set(topics.flatMap((topic) => this.missingPrerequisites(topic.prerequisites))),
    ];
    return `${eligible} of ${topics.length} topics eligible${missing.length ? ' · ' + this.prerequisiteText(missing) : ''}`;
  }
  toggleUnit(id: string): void {
    if (!this.unitAvailable(id)) return;
    this.chosenUnits.update((ids) =>
      ids.includes(id) ? ids.filter((value) => value !== id) : [...ids, id],
    );
    const topicIds = this.scopeTopics().map((topic) => topic.id);
    this.chosenTopics.update((ids) => ids.filter((value) => topicIds.includes(value)));
  }
  toggleTopic(id: string): void {
    const topic = this.scopeTopics().find((value) => value.id === id);
    if (!topic || this.missingPrerequisites(topic.prerequisites).length) return;
    this.chosenTopics.update((ids) =>
      ids.includes(id) ? ids.filter((value) => value !== id) : [...ids, id],
    );
  }
  async begin(): Promise<void> {
    if (!this.store.settings.hasValue() || !this.eligibleTopics().length) return;
    const studied = this.store.settings.value().studiedUnitIds;
    const unitIds =
      this.scope() === 'all'
        ? [...studied]
        : this.chosenUnits().filter((id) => studied.includes(id) && this.unitAvailable(id));
    const topicIds =
      this.scope() === 'all'
        ? []
        : this.chosenTopics().filter((id) =>
            this.store
              .topics()
              .some(
                (topic) =>
                  topic.id === id &&
                  unitIds.includes(topic.unitId) &&
                  !this.missingPrerequisites(topic.prerequisites).length,
              ),
          );
    this.finished.set(false);
    await this.store.start(
      {
        mode: this.mode !== 'review' && this.kind === 'formula' ? 'focused' : this.mode,
        kind: this.mode === 'review' ? 'mixed' : this.kind,
        difficulty: this.mode === 'review' || this.kind === 'formula' ? 'any' : this.difficulty,
        unitIds,
        topicIds,
      },
      Number(this.length),
    );
  }
  async finish(): Promise<void> {
    const previous = this.store.session();
    if (!(await this.store.finish())) return;
    this.finished.set(true);
    if (previous?.status === 'finished' && previous.error) {
      this.mode = previous.request.mode;
      this.kind = previous.request.kind;
      this.difficulty = previous.request.difficulty;
      this.length = previous.length;
      this.scope.set('specific');
      this.chosenUnits.set([...previous.request.unitIds]);
      this.chosenTopics.set([...previous.request.topicIds]);
    }
  }
  async newTopicSession(): Promise<void> {
    const attempt = this.store.current();
    const session = this.store.session();
    if (!attempt || !session) return;
    const topic = this.store.topics().find((value) => value.id === attempt.question.topicId);
    if (!topic || !(await this.store.finish())) return;
    this.mode = 'focused';
    this.kind = 'task';
    this.difficulty = attempt.question.difficulty;
    this.length = session.length;
    this.scope.set('specific');
    this.chosenUnits.set([topic.unitId]);
    this.chosenTopics.set([topic.id]);
  }
}
