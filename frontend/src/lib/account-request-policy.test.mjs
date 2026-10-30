import assert from 'node:assert/strict';
import test from 'node:test';
import { RequestBodyError, clientAddress, readJsonObject } from './account-request-policy.mjs';

function postRequest(body, headers = {}) {
  return new Request('http://127.0.0.1:3000/api/login', {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body,
  });
}

test('reads a JSON object body within the limit', async () => {
  const body = await readJsonObject(postRequest('{"username":"alice","password":"secret"}'), 1024);
  assert.deepEqual(body, { username: 'alice', password: 'secret' });
});

test('refuses bodies over the limit whether or not Content-Length is honest', async () => {
  const oversized = JSON.stringify({ username: 'x'.repeat(2048) });
  await assert.rejects(readJsonObject(postRequest(oversized), 1024), (error) => {
    return error instanceof RequestBodyError && error.reason === 'too_large';
  });
  await assert.rejects(readJsonObject(postRequest(oversized, { 'content-length': '12' }), 1024), (error) => {
    return error instanceof RequestBodyError && error.reason === 'too_large';
  });
});

test('rejects bodies that are not a JSON object', async () => {
  for (const body of ['not json', '[1,2]', '"text"', 'null']) {
    await assert.rejects(readJsonObject(postRequest(body), 1024), (error) => {
      return error instanceof RequestBodyError && error.reason === 'invalid';
    }, body);
  }
});

test('reads a client address only from the header the deployment configures', () => {
  const withRealIp = () => new Request('http://127.0.0.1/', { headers: { 'x-real-ip': '203.0.113.7' } });
  const withForwardedFor = () =>
    new Request('http://127.0.0.1/', { headers: { 'x-forwarded-for': '198.51.100.1, 203.0.113.9' } });

  // Nothing is trusted by default, so a visitor cannot pick its own address.
  assert.equal(clientAddress(withRealIp()), 'unknown');
  assert.equal(clientAddress(withRealIp(), undefined), 'unknown');
  assert.equal(clientAddress(withForwardedFor(), ''), 'unknown');

  assert.equal(clientAddress(withRealIp(), 'x-real-ip'), '203.0.113.7');
  // The proxy appends, so the last entry is the one it observed.
  assert.equal(clientAddress(withForwardedFor(), 'x-forwarded-for'), '203.0.113.9');
  // A header other than the configured one is ignored.
  assert.equal(clientAddress(withRealIp(), 'x-forwarded-for'), 'unknown');
  assert.equal(clientAddress(withForwardedFor(), 'x-real-ip'), 'unknown');
  // Unsupported configuration values trust nothing.
  assert.equal(clientAddress(withRealIp(), 'cf-connecting-ip'), 'unknown');
});
