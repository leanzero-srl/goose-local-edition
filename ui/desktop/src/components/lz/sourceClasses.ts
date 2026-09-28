import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import ts from 'typescript';

/** One class token a source file hands to the DOM, and where. */
export interface ClassUse {
  file: string;
  line: number;
  token: string;
  /** The class string it came from (one literal or one template), numbered across the scan. */
  literal: number;
}

const SRC = resolve(__dirname, '../..');

/** Calls whose string arguments are class lists. */
const CLASS_HELPERS = new Set(['cx', 'cn', 'clsx', 'classNames', 'twMerge', 'twJoin', 'cva']);

/**
 * A JSX attribute or a binding whose value is a class list: `className`, `iconClassName`,
 * `baseClasses`, `baseStyles`, `variants`, `CARD_CLASS`. `errorClass` is a kind, not a class list.
 */
const CLASS_NAME =
  /^(class|className|classNames|classes|cls|styles|variants)$|[a-z0-9](ClassName|ClassNames|Classes|Styles|Variants)$|(^|_)CLASS(ES|NAME)?$/;

/** cva's variant selections, not class lists. */
const VARIANT_SELECTIONS = new Set(['defaultVariants', 'compoundVariants']);

function isClassName(name: ts.Node | undefined): boolean {
  if (!name) return false;
  if (ts.isIdentifier(name) || ts.isStringLiteral(name)) return CLASS_NAME.test(name.text);
  return false;
}

function propertyName(node: ts.PropertyAssignment): string | null {
  return ts.isIdentifier(node.name) || ts.isStringLiteral(node.name) ? node.name.text : null;
}

/**
 * The walk from a string literal up to its class context may pass only through nodes that keep
 * the literal a class list (a branch of a ternary, an `&&`, a template span, an array, a helper's
 * argument). A comparison operand, a translation key or an index is not a class, so the walk
 * stops there and the literal is skipped.
 */
function classContext(node: ts.Node): boolean {
  let child: ts.Node = node;
  let parent = node.parent;
  while (parent) {
    if (ts.isJsxAttribute(parent)) return isClassName(parent.name);
    if (ts.isVariableDeclaration(parent))
      return parent.initializer === child && isClassName(parent.name);
    if (ts.isPropertyAssignment(parent)) {
      if (VARIANT_SELECTIONS.has(propertyName(parent) ?? '')) return false;
      if (isClassName(parent.name) && parent.initializer === child) return true;
      // clsx object syntax: `cx({ 'a b': on })` — the key is the class list.
      if (parent.name === child) {
        const call = parent.parent?.parent;
        return !!call && ts.isCallExpression(call) && isHelperCall(call);
      }
    } else if (ts.isCallExpression(parent)) {
      return isHelperCall(parent) && parent.arguments.includes(child as ts.Expression);
    } else if (ts.isConditionalExpression(parent)) {
      if (parent.condition === child) return false;
    } else if (ts.isBinaryExpression(parent)) {
      const op = parent.operatorToken.kind;
      const passes =
        op === ts.SyntaxKind.AmpersandAmpersandToken ||
        op === ts.SyntaxKind.BarBarToken ||
        op === ts.SyntaxKind.QuestionQuestionToken ||
        op === ts.SyntaxKind.PlusToken;
      if (!passes) return false;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken && parent.left === child) return false;
    } else if (
      !(
        ts.isParenthesizedExpression(parent) ||
        ts.isTemplateSpan(parent) ||
        ts.isTemplateExpression(parent) ||
        ts.isArrayLiteralExpression(parent) ||
        ts.isJsxExpression(parent) ||
        ts.isAsExpression(parent) ||
        ts.isSatisfiesExpression(parent) ||
        ts.isObjectLiteralExpression(parent)
      )
    ) {
      return false;
    }
    child = parent;
    parent = parent.parent;
  }
  return false;
}

function isHelperCall(call: ts.CallExpression): boolean {
  const callee = call.expression;
  return ts.isIdentifier(callee) && CLASS_HELPERS.has(callee.text);
}

/**
 * Whole tokens of a class string with their offsets; a token touching an interpolation
 * (`bg-${tone}`) is partial and dropped.
 */
function tokens(text: string, openLeft: boolean, openRight: boolean): Array<[string, number]> {
  const parts = [...text.matchAll(/\S+/g)].map((m): [string, number] => [m[0], m.index ?? 0]);
  if (openLeft && parts.length > 0 && parts[0][1] === 0) parts.shift();
  const last = parts[parts.length - 1];
  if (openRight && last && last[1] + last[0].length === text.length) parts.pop();
  return parts;
}

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && entry.name !== 'test') sourceFiles(path, out);
    } else if (
      /\.(ts|tsx)$/.test(entry.name) &&
      !/\.(test|spec)\.tsx?$/.test(entry.name) &&
      !entry.name.endsWith('.d.ts')
    ) {
      out.push(path);
    }
  }
  return out;
}

/** Every class token ui/desktop/src hands to the DOM through a className, a class helper or a class-named binding. */
export function sourceClassUses(root: string = SRC): ClassUse[] {
  const uses: ClassUse[] = [];
  let literal = 0;
  for (const path of sourceFiles(root)) {
    const text = readFileSync(path, 'utf8');
    const kind = path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
    const sf = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, kind);
    const file = relative(root, path);
    // The literal's text starts one character in: after the quote, the backtick or the `}`.
    const add = (node: ts.Node, found: Array<[string, number]>) => {
      const start = node.getStart(sf) + 1;
      for (const [token, offset] of found) {
        const line = sf.getLineAndCharacterOfPosition(start + offset).line + 1;
        uses.push({ file, line, token, literal });
      }
    };
    const visit = (node: ts.Node) => {
      if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
        if (classContext(node)) {
          literal++;
          add(node, tokens(node.text, false, false));
        }
      } else if (ts.isTemplateExpression(node)) {
        if (classContext(node)) {
          literal++;
          add(node, tokens(node.head.text, false, true));
          node.templateSpans.forEach((span, i) => {
            const last = i === node.templateSpans.length - 1;
            add(span.literal, tokens(span.literal.text, true, !last));
          });
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(sf);
  }
  return uses;
}
