'use strict';
const assert = require('node:assert/strict');
const {test} = require('node:test');
const {createKvs, LIMITS, QUERY_LAG_MS} = require('../lib/kvs.cjs');

const T0 = 1_800_000_000_000;
const ENTITIES = [{name: 'row', attributes: {n: {type: 'integer'}}, indexes: ['n']}];
function fixture() {
  let time = T0;
  const kvs = createKvs({entities: ENTITIES, now: () => time, floor: () => T0});
  assert.equal(kvs.handle('/api/v1/entity/set', {entityName: 'row', key: 'first', value: {n: 1}}).status, 204);
  assert.equal(kvs.handle('/api/v1/entity/set', {entityName: 'row', key: 'occupied', value: {n: 9}}).status, 204);
  assert.equal(kvs.handle('/api/v1/set', {key: 'pending', value: {sprints: ['old']}}).status, 204);
  time += 1_000;
  assert.equal(kvs.handle('/api/v1/set', {key: 'history', value: 'before'}, {t: time}).status, 204);
  return {kvs, setTime: value => {time = value;}};
}

const first = {entityName: 'row', key: 'first', value: {n: 2}};
const pending = {key: 'pending', value: {sprints: ['new']}};
const failures = [
  ['late entity type', {entityName: 'row', key: 'second', value: {n: 'invalid'}}, 'INCORRECT_PROPERTY_TYPE'],
  ['late key conflict', {entityName: 'row', key: 'occupied', value: {n: 2}, options: {keyPolicy: 'FAIL_IF_EXISTS'}}, 'KEY_CONFLICT'],
  ['late TTL', {key: 'second', value: 'ok', options: {ttl: {value: 0, unit: 'SECONDS'}}}, 'INVALID_TTL'],
  ['late oversize value', {key: 'second', value: 'x'.repeat(LIMITS.valueBytes + 1)}, 'MAX_SIZE'],
  ['late overdeep value', {key: 'second', value: Array.from({length: LIMITS.depth + 1}).reduce(value => ({child: value}), 'end')}, 'MAX_DEPTH'],
  ['late condition', {entityName: 'row', key: 'occupied', value: {n: 2}, conditions: {and: [{property: 'n', condition: 'EQUAL_TO', values: [-1]}]}}, 'CONDITIONAL_CHECK_FAILED'],
];
for (const [label, bad, code] of failures) {
  test(`transaction refuses ${label} without changing values, metadata or stale query history`, () => {
    const {kvs, setTime} = fixture();
    const before = structuredClone(kvs.dump());
    const result = kvs.handle('/api/v1/transaction', {set: [first, pending, bad], delete: [{key: 'history'}]}, {t: T0 + 2_000});
    assert.equal(result.body.code, code);
    assert.deepEqual(kvs.dump(), before);
    assert.equal(kvs.handle('/api/v1/entity/get', {entityName: 'row', key: 'first'}).body.value.n, 1);
    assert.deepEqual(kvs.handle('/api/v1/get', {key: 'pending'}).body.value, {sprints: ['old']});
    setTime(T0 + 2_000 + QUERY_LAG_MS);
    assert.equal(kvs.handle('/api/v1/query', {}).body.data.find(item => item.key === 'history').value, 'before');
  });
}

test('a serialization failure after valid operations also leaves the entire transaction unchanged', () => {
  const {kvs} = fixture();
  const before = structuredClone(kvs.dump());
  const cyclic = {}; cyclic.self = cyclic;
  assert.throws(() => kvs.handle('/api/v1/transaction', {set: [first, pending, {key: 'second', value: cyclic}]}), TypeError);
  assert.deepEqual(kvs.dump(), before);
});

test('a successful mixed transaction commits entity and KV writes and deletes at the same query boundary', () => {
  const {kvs, setTime} = fixture();
  const result = kvs.handle('/api/v1/transaction', {set: [first, pending], delete: [{key: 'history'}, {entityName: 'row', key: 'occupied'}]}, {t: T0 + 2_000});
  assert.equal(result.status, 204);
  assert.equal(kvs.handle('/api/v1/entity/get', {entityName: 'row', key: 'first'}).body.value.n, 2);
  assert.deepEqual(kvs.handle('/api/v1/get', {key: 'pending'}).body.value, {sprints: ['new']});
  assert.equal(kvs.handle('/api/v1/get', {key: 'history'}).status, 404);
  assert.equal(kvs.handle('/api/v1/entity/get', {entityName: 'row', key: 'occupied'}).status, 404);
  const query = () => kvs.handle('/api/v1/query', {}, {t: T0 + 2_000}).body.data;
  assert.deepEqual(query().find(item => item.key === 'pending').value, {sprints: ['old']});
  setTime(T0 + 2_000 + QUERY_LAG_MS);
  const fresh = kvs.handle('/api/v1/query', {}, {t: T0 + 2_000 + QUERY_LAG_MS}).body.data;
  assert.deepEqual(fresh.find(item => item.key === 'pending').value, {sprints: ['new']});
  assert.equal(fresh.some(item => item.key === 'history'), false);
});

test('batch operations keep their documented independent per-item outcomes', () => {
  const {kvs} = fixture();
  const result = kvs.handle('/api/v1/batch/set', [first, failures[0][1]], {t: T0 + 2_000});
  assert.equal(result.status, 200);
  assert.deepEqual(result.body.successfulKeys, [{key: 'first', entityName: 'row'}]);
  assert.equal(result.body.failedKeys[0].error.code, 'INCORRECT_PROPERTY_TYPE');
  assert.equal(kvs.handle('/api/v1/entity/get', {entityName: 'row', key: 'first'}).body.value.n, 2);
});

for (const path of ['set', 'entity/set', 'secret/set']) {
  test(`single ${path} preserves return values, creation time and expired-key policy`, () => {
    let time = T0;
    const kvs = createKvs({entities: ENTITIES, now: () => time, floor: () => T0});
    const identity = {key: 'same', ...(path.startsWith('entity/') ? {entityName: 'row'} : {})};
    const write = (n, options) => kvs.handle(`/api/v1/${path}`, {...identity, value: {n}, options}, {t: time});
    const latest = write(1, {ttl: {value: 1, unit: 'SECONDS'}, returnValue: 'LATEST'});
    assert.equal(latest.status, 200);
    assert.deepEqual(latest.body.value, {n: 1});
    assert.equal(latest.body.createdAt, T0);
    assert.equal(latest.body.updatedAt, T0);
    assert.equal(latest.body.expireTime, new Date(T0 + 1_000).toISOString());
    time += 100;
    const previous = write(2, {ttl: {value: 1, unit: 'SECONDS'}, returnValue: 'PREVIOUS'});
    assert.equal(previous.status, 200);
    assert.deepEqual(previous.body, latest.body);
    const before = structuredClone(kvs.dump());
    assert.equal(write(3, {keyPolicy: 'FAIL_IF_EXISTS'}).body.code, 'KEY_CONFLICT');
    assert.deepEqual(kvs.dump(), before);
    time = T0 + 1_100;
    const replaced = write(3, {keyPolicy: 'FAIL_IF_EXISTS', returnValue: 'LATEST'});
    assert.equal(replaced.status, 200);
    assert.deepEqual(replaced.body.value, {n: 3});
    assert.equal(replaced.body.createdAt, time);
    assert.equal(replaced.body.updatedAt, time);
    assert.equal(Object.hasOwn(replaced.body, 'expireTime'), false);
  });
}
