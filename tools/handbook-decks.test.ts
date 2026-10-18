import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import katex from 'katex';
import type { Question } from '@math/contract';

const decks = ['formulas-mst124.json', 'formulas-mst125.json'].flatMap(
  (file) =>
    JSON.parse(readFileSync(new URL(`../data/${file}`, import.meta.url), 'utf8')) as Question[],
);

test('handbook card mathematics renders as accessible math without broken delimiters', () => {
  for (const card of decks) {
    for (const field of ['prompt', 'answer', 'explanation'] as const) {
      const value = card[field];
      const expressions = /\$\$([\s\S]+?)\$\$|\$([^$\n]+?)\$/g;
      assert.equal(
        value.replace(expressions, '').includes('$'),
        false,
        `${card.id}: ${field} has an unmatched math delimiter`,
      );
      for (const match of value.matchAll(expressions)) {
        const html = katex.renderToString(match[1] ?? match[2], {
          displayMode: match[1] !== undefined,
          throwOnError: false,
          strict: 'error',
          trust: false,
          maxSize: 20,
          maxExpand: 200,
        });
        assert.match(
          html,
          /<math(?:\s|>)/,
          `${card.id}: ${field} must render as MathML rather than a KaTeX error: ${match[0]}`,
        );
      }
    }
  }
});
