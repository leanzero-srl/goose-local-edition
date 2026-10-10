// A tiny DOM builder: the widget and the sprint action stay well inside the boot budget (§17) without a UI library.
// Listeners are attached with addEventListener, never as inline handlers, and no style attribute is ever written,
// so the default Custom UI content security policy holds.
const SVG_NS = 'http://www.w3.org/2000/svg';

function build(el, attrs, children) {
  for (const [name, value] of Object.entries(attrs ?? {})) {
    if (value === undefined || value === null || value === false) continue;
    if (name.startsWith('on') && typeof value === 'function') el.addEventListener(name.slice(2).toLowerCase(), value);
    else el.setAttribute(name, value === true ? '' : String(value));
  }
  for (const child of children.flat(Infinity)) {
    if (child === undefined || child === null || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

export const h = (tag, attrs, ...children) => build(document.createElement(tag), attrs, children);
export const s = (tag, attrs, ...children) => build(document.createElementNS(SVG_NS, tag), attrs, children);

export function replace(parent, ...children) {
  parent.replaceChildren(...children.flat(Infinity).filter((c) => c !== undefined && c !== null && c !== false));
}
