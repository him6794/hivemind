import assert from 'node:assert/strict';
import test from 'node:test';
import {
  REGISTER_STEPS,
  firstInvalidRegisterStep,
  stepForRegisterError,
  validateRegisterStep,
} from './register-flow-policy.mjs';

const valid = {
  email: 'alice@example.com',
  username: 'alice',
  password: 'correct horse',
  confirm: 'correct horse',
};

test('sign-up asks for the email, then the username, then the password, and ends with a review', () => {
  assert.deepEqual(REGISTER_STEPS, ['email', 'username', 'password', 'review']);
});

test('the email step accepts an address and refuses one that is not valid', () => {
  assert.deepEqual(validateRegisterStep('email', valid), { ok: true });
  assert.deepEqual(validateRegisterStep('email', { ...valid, email: 'not-an-address' }), {
    ok: false,
    code: 'invalid_email',
  });
  assert.deepEqual(validateRegisterStep('email', { ...valid, email: '' }), { ok: false, code: 'invalid_email' });
});

test('the username step needs three characters once the spaces at the edges are removed', () => {
  assert.deepEqual(validateRegisterStep('username', { ...valid, username: 'abc' }), { ok: true });
  assert.deepEqual(validateRegisterStep('username', { ...valid, username: 'ab' }), {
    ok: false,
    code: 'username_too_short',
  });
  assert.deepEqual(validateRegisterStep('username', { ...valid, username: '  ab  ' }), {
    ok: false,
    code: 'username_too_short',
  });
});

test('the password step applies the password rule, then checks that the two entries match', () => {
  assert.deepEqual(validateRegisterStep('password', valid), { ok: true });
  assert.deepEqual(validateRegisterStep('password', { ...valid, password: 'short', confirm: 'short' }), {
    ok: false,
    code: 'password_too_short',
  });
  assert.deepEqual(validateRegisterStep('password', { ...valid, password: 'a'.repeat(73), confirm: 'a'.repeat(73) }), {
    ok: false,
    code: 'password_too_long',
  });
  // Chinese characters take three bytes each, so 25 of them are 75 bytes.
  assert.deepEqual(validateRegisterStep('password', { ...valid, password: '中'.repeat(25), confirm: '中'.repeat(25) }), {
    ok: false,
    code: 'password_too_long',
  });
  assert.deepEqual(validateRegisterStep('password', { ...valid, password: ' correct horse', confirm: ' correct horse' }), {
    ok: false,
    code: 'password_edge_whitespace',
  });
  assert.deepEqual(validateRegisterStep('password', { ...valid, confirm: 'correct horsf' }), {
    ok: false,
    code: 'password_mismatch',
  });
});

test('the review step has no fields of its own to check', () => {
  assert.deepEqual(validateRegisterStep('review', { email: '', username: '', password: '', confirm: '' }), { ok: true });
});

test('the first step with a problem is reported, so a visitor is taken to the earliest mistake', () => {
  assert.equal(firstInvalidRegisterStep(valid), null);
  assert.deepEqual(firstInvalidRegisterStep({ ...valid, username: 'a' }), { step: 'username', code: 'username_too_short' });
  assert.deepEqual(firstInvalidRegisterStep({ ...valid, email: 'bad', username: 'a' }), {
    step: 'email',
    code: 'invalid_email',
  });
});

test('a refusal from the server goes back to the step whose field caused it', () => {
  assert.equal(stepForRegisterError('email_in_use'), 'email');
  assert.equal(stepForRegisterError('invalid_email'), 'email');
  assert.equal(stepForRegisterError('username_taken'), 'username');
  assert.equal(stepForRegisterError('username_too_short'), 'username');
  assert.equal(stepForRegisterError('password_mismatch'), 'password');
  assert.equal(stepForRegisterError('password_too_short'), 'password');
  // Verification and network problems stay on the review step, where the visitor can try again.
  assert.equal(stepForRegisterError('captcha_failed'), 'review');
  assert.equal(stepForRegisterError('request_failed'), 'review');
});
