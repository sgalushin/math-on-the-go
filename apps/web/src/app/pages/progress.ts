import { Component, computed, inject, signal } from '@angular/core';
import { DatePipe, PercentPipe } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { Store, messageOf } from '../core/store';
import type { Difficulty, TopicProgress } from '@math/contract';

@Component({
  standalone: true,
  imports: [RouterLink, DatePipe, PercentPipe, FormsModule],
  template: `
    <header class="page-heading">
      <p class="eyebrow">Evidence, not a mastery score</p>
      <h1>Progress</h1>
      <p class="lede">
        Task outcomes and flashcard recall, kept separate. Your own practice is the only data here.
      </p>
    </header>
    @if (store.progress.isLoading()) {
      <p class="status" role="status">Gathering your practice record…</p>
    }
    @if (store.progress.error()) {
      <div class="error" role="alert">
        {{ messageOf(store.progress.error()) }}
        <button class="text-button" (click)="store.progress.reload()">Retry</button>
      </div>
    }
    @if (store.progress.hasValue()) {
      @let p = store.progress.value();
      <section class="panel">
        <div class="section-heading">
          <h2>Task practice</h2>
          <span class="eyebrow">{{ p.completedTasks }} completed</span>
        </div>
        <div class="stat-grid">
          <div>
            <strong>{{ p.independent }}</strong
            ><span>independent</span>
          </div>
          <div>
            <strong>{{ p.assisted }}</strong
            ><span>with assistance</span>
          </div>
          <div>
            <strong>{{ p.incorrect }}</strong
            ><span>incorrect</span>
          </div>
        </div>
        <p class="small muted">
          {{ p.coveredTopics }} of {{ p.totalTopics }} topics practised · {{ p.studiedUnits }} units
          marked studied. Self-assessed reasoning is counted separately below. Flagged questions are
          excluded.
        </p>
      </section>
      <section class="panel flashcard-summary">
        <div>
          <p class="eyebrow">Spaced recall · flashcards only</p>
          <h2>Flashcard recall</h2>
        </div>
        <div class="stat-grid">
          <div>
            <strong>{{ p.dueFlashcards }}</strong
            ><span>due now</span>
          </div>
          <div>
            <strong>{{ p.newFlashcards }}</strong
            ><span>new cards</span>
          </div>
          <div>
            <strong>{{ p.reviewedFlashcards }}</strong
            ><span>reviews completed</span>
          </div>
        </div>
        <a routerLink="/practice" [queryParams]="{ mode: 'review' }" class="text-link"
          >Review flashcards →</a
        >
      </section>
      <section class="section">
        <div class="section-heading">
          <h2>Topic by topic</h2>
          <label class="sr-only" for="progress-module">Filter module</label
          ><select
            id="progress-module"
            class="compact-select"
            [ngModel]="module()"
            (ngModelChange)="module.set($event)"
          >
            <option value="">Both modules</option>
            <option value="MST124">MST124</option>
            <option value="MST125">MST125</option>
          </select>
        </div>
        <p class="small muted">
          Independent accuracy = independent ÷ (independent + assisted + incorrect), for
          automatically assessed tasks. Recent compares the latest 10 eligible attempts with the
          previous 10. Small samples are not mastery.
        </p>
        @for (topic of topics(); track topic.id) {
          @let row = byTopic()[topic.id];
          <details class="topic-progress">
            <summary>
              <div>
                <p class="eyebrow">{{ topic.label.split(' · ').slice(0, 2).join(' · ') }}</p>
                <h3>{{ topic.title }}</h3>
              </div>
              <span class="topic-count"
                >{{ row?.attempts ?? 0 }} task attempts <span aria-hidden="true">+</span></span
              >
            </summary>
            @if (row && row.attempts) {
              <div class="topic-breakdown">
                <div class="count-pills">
                  <span>{{ row.independent }} independent</span
                  ><span>{{ row.assisted }} assisted</span><span>{{ row.incorrect }} incorrect</span
                  ><span>{{ row.selfAssessed }} self-assessed</span
                  ><span>{{ row.skipped }} skipped</span><span>{{ row.unfinished }} unfinished</span
                  ><span>{{ row.flagged }} flagged</span>
                </div>
                <div class="trend">
                  <strong>{{
                    row.recentAccuracy === null
                      ? 'No auto-scored sample yet'
                      : (row.recentAccuracy | percent: '1.0-0') + ' recent independent accuracy'
                  }}</strong>
                  <p class="small">
                    {{ sampleSize(row.independent, row.assisted, row.incorrect) }} recent
                    automatically assessed attempts.
                    @if (row.previousAccuracy !== null) {
                      Previous sample: {{ row.previousAccuracy | percent: '1.0-0' }} ·
                      {{ previousSize(row.independent, row.assisted, row.incorrect) }} attempts.
                    }
                    @if (row.recentAccuracy !== null && row.previousAccuracy !== null) {
                      <span>{{
                        row.recentAccuracy > row.previousAccuracy
                          ? 'Improving across these samples.'
                          : row.recentAccuracy < row.previousAccuracy
                            ? 'Lower across these samples.'
                            : 'Unchanged across these samples.'
                      }}</span>
                    }
                  </p>
                </div>
                <div class="table-scroll">
                  <table>
                    <caption class="sr-only">
                      Task counts by difficulty
                    </caption>
                    <thead>
                      <tr>
                        <th>Difficulty</th>
                        <th>Attempts</th>
                        <th>Independent</th>
                        <th>Assisted</th>
                        <th>Incorrect</th>
                      </tr>
                    </thead>
                    <tbody>
                      @for (level of levels; track level) {
                        <tr>
                          <th>{{ level }}</th>
                          <td>{{ row.byDifficulty[level].attempts }}</td>
                          <td>{{ row.byDifficulty[level].independent }}</td>
                          <td>{{ row.byDifficulty[level].assisted }}</td>
                          <td>{{ row.byDifficulty[level].incorrect }}</td>
                        </tr>
                      }
                    </tbody>
                  </table>
                </div>
                <p class="small muted">
                  Last practised:
                  {{ row.lastPractised ? (row.lastPractised | date: 'mediumDate') : 'Not yet' }}
                </p>
                <a routerLink="/history" class="text-link">Explore your attempt history →</a>
              </div>
            } @else {
              <p class="empty-topic">
                No task attempts yet.
                <a routerLink="/practice">Choose your session scope in Practice →</a>
              </p>
            }
          </details>
        }
        @if (!p.totalAttempts) {
          <div class="empty">
            <h2>No practice record yet</h2>
            @if (store.settings.hasValue() && store.settings.value().studiedUnitIds.length) {
              <p>Your first session will start your record.</p>
              <a routerLink="/practice" class="button">Start a session</a>
            } @else {
              <p>Mark what you have covered in your profile, then start a session.</p>
              <a routerLink="/profile/units" class="button">Add studied units</a>
            }
          </div>
        }
      </section>
    }
  `,
})
export class ProgressPage {
  readonly store = inject(Store);
  readonly messageOf = messageOf;
  readonly module = signal('');
  readonly levels: Difficulty[] = ['quick', 'standard', 'stretch'];
  readonly topics = computed(() =>
    this.store
      .topics()
      .filter(
        (topic) =>
          !this.module() ||
          this.store.units().find((unit) => unit.id === topic.unitId)?.module === this.module(),
      ),
  );
  readonly byTopic = computed<Partial<Record<string, TopicProgress>>>(() =>
    Object.fromEntries(
      (this.store.progress.hasValue() ? this.store.progress.value().topics : []).map((topic) => [
        topic.topicId,
        topic,
      ]),
    ),
  );
  constructor() {
    if (!this.store.progress.isLoading()) this.store.progress.reload();
  }
  sampleSize(independent: number, assisted: number, incorrect: number): number {
    return Math.min(10, independent + assisted + incorrect);
  }
  previousSize(independent: number, assisted: number, incorrect: number): number {
    return Math.min(10, Math.max(0, independent + assisted + incorrect - 10));
  }
}
