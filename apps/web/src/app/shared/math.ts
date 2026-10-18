import { Component, ElementRef, afterRenderEffect, input, viewChild } from '@angular/core';
import katex from 'katex';
import type { Graph } from '@math/contract';

@Component({
  selector: 'math-text',
  standalone: true,
  template: '<span #content class="math-content"></span>',
  host: { class: 'math-text' },
})
export class MathText {
  readonly text = input.required<string>();
  private readonly content = viewChild.required<ElementRef<HTMLElement>>('content');
  constructor() {
    afterRenderEffect(() => {
      const root = this.content().nativeElement;
      const text = this.text();
      root.replaceChildren();
      const expression = /\$\$([\s\S]+?)\$\$|\$([^$\n]+?)\$/g;
      let end = 0;
      for (const match of text.matchAll(expression)) {
        root.append(document.createTextNode(text.slice(end, match.index)));
        const node = document.createElement(match[1] !== undefined ? 'div' : 'span');
        try {
          katex.render(match[1] ?? match[2] ?? '', node, {
            displayMode: match[1] !== undefined,
            throwOnError: false,
            trust: false,
            strict: 'warn',
            maxExpand: 200,
            maxSize: 20,
          });
        } catch {
          node.textContent = match[0];
        }
        root.append(node);
        end = (match.index ?? 0) + match[0].length;
      }
      root.append(document.createTextNode(text.slice(end)));
    });
  }
}
@Component({
  selector: 'math-graph',
  standalone: true,
  template: `
    <figure class="graph">
      <svg
        viewBox="0 0 400 300"
        role="img"
        aria-label="Question graph"
        [attr.aria-describedby]="descriptionId"
      >
        <rect x="42" y="18" width="340" height="242" fill="var(--panel)" stroke="var(--fg)" />
        @for (tick of ticks; track tick) {
          <line
            [attr.x1]="42 + tick * 85"
            y1="18"
            [attr.x2]="42 + tick * 85"
            y2="260"
            stroke="var(--line)"
            stroke-dasharray="3 5"
          />
          <line
            x1="42"
            [attr.y1]="18 + tick * 60.5"
            x2="382"
            [attr.y2]="18 + tick * 60.5"
            stroke="var(--line)"
            stroke-dasharray="3 5"
          />
          <text [attr.x]="42 + tick * 85" y="281" text-anchor="middle">
            {{ number(graph().xMin + (tick * (graph().xMax - graph().xMin)) / 4) }}
          </text>
          <text x="34" [attr.y]="264 - tick * 60.5" text-anchor="end">
            {{ number(graph().yMin + (tick * (graph().yMax - graph().yMin)) / 4) }}
          </text>
        }
        @if (graph().yMin <= 0 && graph().yMax >= 0) {
          <line x1="42" [attr.y1]="y(0)" x2="382" [attr.y2]="y(0)" stroke="var(--fg)" />
        }
        @if (graph().xMin <= 0 && graph().xMax >= 0) {
          <line [attr.x1]="x(0)" y1="18" [attr.x2]="x(0)" y2="260" stroke="var(--fg)" />
        }
        @for (series of graph().series; track $index) {
          <polyline
            [attr.points]="points(series.points)"
            fill="none"
            [attr.stroke]="colors[$index % colors.length]"
            stroke-width="2.5"
            [attr.stroke-dasharray]="$index === 1 ? '7 4' : $index === 2 ? '2 4' : null"
          />
        }
        <text x="391" y="278">x</text>
        <text x="20" y="12">y</text>
      </svg>
      <figcaption [id]="descriptionId">
        @for (series of graph().series; track $index) {
          <span class="graph-key" [style.color]="colors[$index % colors.length]"
            >{{ series.label }}{{ $last ? '' : ' · ' }}</span
          >
        }
        <span class="sr-only"
          >. x ranges from {{ graph().xMin }} to {{ graph().xMax }}; y ranges from
          {{ graph().yMin }} to {{ graph().yMax }}.</span
        >
      </figcaption>
      <details>
        <summary>Graph data</summary>
        @for (series of graph().series; track $index) {
          <p>{{ series.label }}</p>
          <div class="graph-data">
            @for (point of series.points; track $index) {
              <span>({{ number(point.x) }}, {{ number(point.y) }}) </span>
            }
          </div>
        }
      </details>
    </figure>
  `,
})
export class MathGraph {
  readonly graph = input.required<Graph>();
  readonly ticks = [0, 1, 2, 3, 4];
  readonly colors = ['var(--graph-blue)', 'var(--graph-red)', 'var(--graph-green)'];
  readonly descriptionId = `graph-${crypto.randomUUID()}`;
  x(value: number): number {
    const g = this.graph();
    return 42 + ((value - g.xMin) / (g.xMax - g.xMin)) * 340;
  }
  y(value: number): number {
    const g = this.graph();
    return 260 - ((value - g.yMin) / (g.yMax - g.yMin)) * 242;
  }
  points(points: { x: number; y: number }[]): string {
    return points.map((point) => `${this.x(point.x)},${this.y(point.y)}`).join(' ');
  }
  number(value: number): string {
    return Number(value.toPrecision(4)).toString();
  }
}
