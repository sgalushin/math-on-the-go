import { Component, inject } from '@angular/core';
import { DatePipe } from '@angular/common';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { httpResource } from '@angular/common/http';
import type { Attempt } from '@math/contract';
import { Store, messageOf } from '../core/store';
import { AttemptView } from '../shared/attempt-view';
import { MathText } from '../shared/math';

@Component({
  standalone: true,
  imports: [RouterLink, DatePipe, AttemptView, MathText],
  template: `
    <a routerLink="/history" class="back-link">← All attempts</a>
    <header class="page-heading">
      <p class="eyebrow">Saved question snapshot</p>
      <h1>Attempt details</h1>
    </header>
    @if (resource.isLoading()) {
      <p role="status" class="status">Opening attempt…</p>
    }
    @if (resource.error()) {
      <div class="error" role="alert">
        {{ messageOf(resource.error())
        }}<button class="text-button" (click)="resource.reload()">Retry</button>
      </div>
    }
    @if (resource.hasValue()) {
      @let attempt = resource.value();
      <p class="topic-label">
        {{ store.topicName(attempt.question.topicId) }} · {{ attempt.startedAt | date: 'medium' }}
      </p>
      @if (attempt.outcome === 'unfinished' && store.session()?.attemptIds?.at(-1) === attempt.id) {
        <a routerLink="/practice" class="button secondary">Resume this question →</a>
      }
      <attempt-view [attempt]="attempt" />
      <section class="panel timeline-panel">
        <h2>Attempt timeline</h2>
        <ol class="timeline">
          <li>
            <time>{{ attempt.startedAt | date: 'medium' }}</time
            ><strong>Question started</strong>
            <p>{{ attempt.question.kind }} · {{ attempt.question.difficulty }}</p>
          </li>
          @for (submission of attempt.submissions; track $index) {
            <li>
              <time>{{ submission.at | date: 'medium' }}</time
              ><strong
                >Answer {{ $index + 1 }} ·
                {{
                  submission.correct === null
                    ? 'Awaiting self-assessment'
                    : submission.correct
                      ? 'Correct'
                      : 'Incorrect'
                }}</strong
              ><math-text [text]="submission.answer" />
            </li>
          }
          <li>
            <time>{{ attempt.updatedAt | date: 'medium' }}</time
            ><strong>Latest saved snapshot</strong>
            <p>
              {{ attempt.hintsUsed }} hints used ·
              {{ attempt.solutionRevealed ? 'Solution revealed' : 'Solution not revealed' }}
              @if (attempt.rating) {
                · Recall rating: {{ attempt.rating }}
              }
            </p>
            @if (attempt.reported || attempt.question.flagged) {
              <p>Excluded from progress.</p>
            }
            <p class="small muted">
              Hint and reveal counts are saved with this snapshot. Individual action timestamps are
              included in the full history export.
            </p>
          </li>
          @if (attempt.completedAt) {
            <li>
              <time>{{ attempt.completedAt | date: 'medium' }}</time
              ><strong>{{ attempt.outcome.replace('_', ' ') }}</strong>
              <p>
                {{
                  attempt.assessment === 'self'
                    ? 'Self-assessed conceptual reasoning, not an automatic score.'
                    : 'Server-checked assessment.'
                }}
              </p>
            </li>
          }
        </ol>
        <p class="small muted">
          Question ID: {{ attempt.question.id }}<br />Attempt ID: {{ attempt.id }}
        </p>
      </section>
    }
  `,
})
export class AttemptDetailPage {
  readonly store = inject(Store);
  readonly messageOf = messageOf;
  private readonly id = inject(ActivatedRoute).snapshot.paramMap.get('id') ?? '';
  readonly resource = httpResource<Attempt>(() => `/api/attempts/${encodeURIComponent(this.id)}`);
}
