import { Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { DatePipe } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { httpResource } from '@angular/common/http';
import { firstValueFrom } from 'rxjs';
import type { Attempt, HistoryResponse } from '@math/contract';
import { Store, messageOf } from '../core/store';
import { MathText } from '../shared/math';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink, DatePipe, MathText],
  template: `
    <header class="page-heading heading-row">
      <div>
        <p class="eyebrow">Your practice record</p>
        <h1>History</h1>
        <p class="lede">Full question snapshots, your answers, and the help you used.</p>
      </div>
      <button class="button secondary" (click)="exportHistory()" [disabled]="exporting()">
        {{ exporting() ? 'Exporting…' : 'Export all JSON' }}
      </button>
    </header>
    <details class="panel filter-panel" open>
      <summary>
        Filter attempts
        <span>{{ activeCount() ? activeCount() + ' applied' : 'All attempts' }}</span>
      </summary>
      <form (ngSubmit)="apply()">
        <div class="form-grid three">
          <div>
            <label for="history-unit">Unit</label
            ><select
              id="history-unit"
              name="unit"
              [(ngModel)]="unitId"
              (ngModelChange)="topicId = ''"
            >
              <option value="">All units</option>
              @for (unit of store.units(); track unit.id) {
                <option [value]="unit.id">
                  {{ unit.module }} {{ unit.number }} · {{ unit.title }}
                </option>
              }
            </select>
          </div>
          <div>
            <label for="history-topic">Topic</label
            ><select id="history-topic" name="topic" [(ngModel)]="topicId">
              <option value="">All topics</option>
              @for (topic of store.topics(); track topic.id) {
                @if (!unitId || topic.unitId === unitId) {
                  <option [value]="topic.id">{{ topic.label }}</option>
                }
              }
            </select>
          </div>
          <div>
            <label for="history-kind">Kind</label
            ><select id="history-kind" name="kind" [(ngModel)]="kind">
              <option value="">Cards & tasks</option>
              <option value="task">Task</option>
              <option value="flashcard">Flashcard</option>
            </select>
          </div>
          <div>
            <label for="history-outcome">Outcome</label
            ><select id="history-outcome" name="outcome" [(ngModel)]="outcome">
              <option value="">All outcomes</option>
              <option value="unfinished">Unfinished</option>
              <option value="independent">Independent</option>
              <option value="assisted">Assisted</option>
              <option value="incorrect">Incorrect</option>
              <option value="self_assessed">Self-assessed</option>
              <option value="skipped">Skipped</option>
            </select>
          </div>
          <div>
            <label for="history-difficulty">Difficulty</label
            ><select id="history-difficulty" name="difficulty" [(ngModel)]="difficulty">
              <option value="">All difficulties</option>
              <option value="quick">Quick</option>
              <option value="standard">Standard</option>
              <option value="stretch">Stretch</option>
            </select>
          </div>
          <div>
            <label for="history-from">From date</label
            ><input id="history-from" name="from" type="date" [(ngModel)]="from" />
          </div>
          <div>
            <label for="history-to">Through date</label
            ><input id="history-to" name="to" type="date" [(ngModel)]="to" />
          </div>
        </div>
        <div class="form-actions">
          <button class="button" type="submit">Apply filters</button
          ><button class="text-button" type="button" (click)="reset()">Clear all</button>
        </div>
      </form>
    </details>
    @if (error()) {
      <p role="alert" class="error">{{ error() }}</p>
    }
    @if (history.error()) {
      <div class="error" role="alert">
        {{ messageOf(history.error()) }}
        <button class="text-button" (click)="history.reload()">Retry</button>
      </div>
    }
    @if (history.isLoading()) {
      <p role="status" class="status">Loading attempts…</p>
    }
    <div class="history-list">
      @for (attempt of attempts(); track attempt.id) {
        <a [routerLink]="['/history', attempt.id]" class="history-row"
          ><div class="history-date">
            <strong>{{ attempt.startedAt | date: 'dd' }}</strong
            ><span>{{ attempt.startedAt | date: 'MMM' }}</span>
          </div>
          <div class="history-body">
            <div class="question-meta">
              <span>{{ attempt.question.kind }}</span
              ><span>{{ attempt.question.difficulty }}</span>
              @if (attempt.reported || attempt.question.flagged) {
                <span>Excluded</span>
              }
            </div>
            <h2>{{ store.topicName(attempt.question.topicId) }}</h2>
            <div class="history-prompt"><math-text [text]="attempt.question.prompt" /></div>
            <span class="outcome-tag" [class.independent]="attempt.outcome === 'independent'">{{
              attempt.outcome.replace('_', ' ')
            }}</span>
          </div>
          <span aria-hidden="true">↗</span></a
        >
      }
    </div>
    @if (!history.isLoading() && !history.error() && !attempts().length) {
      <div class="empty">
        <h2>No attempts here yet.</h2>
        <p>
          {{
            activeCount()
              ? 'Try a wider set of filters, or start a new session.'
              : 'Your first question will start your working history.'
          }}
        </p>
        <a class="button secondary" routerLink="/practice">Find a question</a>
      </div>
    }
    @if (history.hasValue() && history.value().nextCursor) {
      <button class="button secondary full" [disabled]="history.isLoading()" (click)="loadMore()">
        Load earlier attempts
      </button>
    }
  `,
})
export class HistoryPage {
  readonly store = inject(Store);
  readonly messageOf = messageOf;
  unitId = '';
  topicId = '';
  kind = '';
  outcome = '';
  difficulty = '';
  from = '';
  to = '';
  private readonly filters = signal('');
  private readonly cursor = signal('');
  readonly activeCount = signal(0);
  readonly attempts = signal<Attempt[]>([]);
  readonly error = signal('');
  readonly exporting = signal(false);
  readonly query = computed(
    () =>
      `/api/history?limit=30${this.filters()}${this.cursor() ? '&cursor=' + encodeURIComponent(this.cursor()) : ''}`,
  );
  readonly history = httpResource<HistoryResponse>(() => this.query());
  constructor() {
    effect(() => {
      if (!this.history.isLoading() && this.history.hasValue()) {
        const result = this.history.value();
        const previous = this.cursor() ? untracked(this.attempts) : [];
        this.attempts.set([
          ...new Map(
            [...previous, ...result.attempts].map((attempt) => [attempt.id, attempt]),
          ).values(),
        ]);
      }
    });
  }
  apply(): void {
    this.error.set('');
    if (this.from && this.to && this.from > this.to) {
      this.error.set('The end date must be on or after the start date.');
      return;
    }
    const values: Record<string, string> = {
      unitId: this.unitId,
      topicId: this.topicId,
      kind: this.kind,
      outcome: this.outcome,
      difficulty: this.difficulty,
      from: this.from ? String(new Date(this.from + 'T00:00:00').getTime()) : '',
      to: this.to ? String(new Date(this.to + 'T23:59:59.999').getTime()) : '',
    };
    const params = new URLSearchParams(Object.entries(values).filter(([, value]) => !!value));
    this.activeCount.set([...params].length);
    this.attempts.set([]);
    this.cursor.set('');
    this.filters.set(params.size ? '&' + params.toString() : '');
    this.history.reload();
  }
  reset(): void {
    this.unitId = '';
    this.topicId = '';
    this.kind = '';
    this.outcome = '';
    this.difficulty = '';
    this.from = '';
    this.to = '';
    this.apply();
  }
  loadMore(): void {
    if (this.history.hasValue()) this.cursor.set(this.history.value().nextCursor ?? '');
  }
  async exportHistory(): Promise<void> {
    if (this.exporting()) return;
    this.exporting.set(true);
    this.error.set('');
    try {
      const data = await firstValueFrom(
        this.store.http.get('/api/history/export', { responseType: 'blob' }),
      );
      const url = URL.createObjectURL(data);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `math-history-${new Date().toISOString().slice(0, 10)}.json`;
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (error) {
      this.error.set(messageOf(error));
    } finally {
      this.exporting.set(false);
    }
  }
}
