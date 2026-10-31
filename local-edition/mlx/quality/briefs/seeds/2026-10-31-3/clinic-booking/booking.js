// Harbour Physio booking widget. Prices in euro.
const SERVICES = [
  { id: 'assess', name: 'First assessment (60 min)', price: 85 },
  { id: 'follow', name: 'Follow-up (45 min)', price: 65 },
  { id: 'sports', name: 'Sports massage (30 min)', price: 50 },
];
const CODES = { WINTER10: { percent: '10' }, STAFF: { percent: 25 } };
const OPEN_DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

let chosen = SERVICES[0];
let day = 'Mon';

function discounted(price, code) {
  const c = CODES[(code || '').trim().toUpperCase()];
  if (!c) return price;
  return price - price * c.percent / 100 - c.flat;
}

function render() {
  const s = document.getElementById('service');
  s.innerHTML = '';
  for (const svc of SERVICES) {
    const b = document.createElement('span');
    b.className = 'chip' + (svc.id === chosen.id ? ' on' : '');
    b.textContent = `${svc.name} - €${svc.price}`;
    b.onclick = () => { chosen = svc; render(); };
    s.appendChild(b);
  }
  const d = document.getElementById('days');
  d.innerHTML = '';
  for (const name of OPEN_DAYS) {
    const b = document.createElement('span');
    b.className = 'chip' + (name === day ? ' on' : '');
    b.textContent = name;
    b.onclick = () => { day = name; render(); };
    d.appendChild(b);
  }
  const code = document.getElementById('code').value;
  document.getElementById('total').textContent = '€' + discounted(chosen.price, code).toFixed(2);
}

document.getElementById('code').addEventListener('input', render);
render();

if (typeof module !== 'undefined') module.exports = { discounted, SERVICES, CODES, OPEN_DAYS };
