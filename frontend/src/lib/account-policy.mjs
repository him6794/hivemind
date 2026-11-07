const ACCOUNT_MESSAGES = {
  zh: {
    credentials_required: '請輸入使用者名稱與密碼。',
    login_failed: '使用者名稱或密碼不正確。',
    registration_failed: '無法建立帳號，請檢查使用者名稱與電子郵件後再試。',
    username_taken: '這個使用者名稱已經有人用過了。',
    email_in_use: '這個電子郵件已經有人用過了。',
    username_too_short: '使用者名稱至少需要 3 個字元。',
    invalid_email: '請輸入有效的電子郵件地址。',
    password_too_short: '密碼至少需要 8 個字元。',
    password_too_long: '密碼不能超過 72 個位元組（中文字每個約 3 個位元組）。',
    password_edge_whitespace: '密碼的開頭或結尾不能有空白。',
    password_mismatch: '兩次輸入的密碼不一致。',
    password_required: '請輸入目前的密碼。',
    invalid_recipient: '請輸入收款人的使用者名稱。',
    invalid_amount: '請輸入大於 0 的整數 CPT。',
    captcha_required: '請先完成人機驗證。',
    captcha_failed: '人機驗證未通過，請再試一次。',
    captcha_not_configured: '目前無法完成人機驗證，請稍後再試。',
    captcha_unavailable: '目前無法完成人機驗證，請稍後再試。',
    verification_failed: '這個驗證連結無效、已過期，或已被較新的驗證信取代。請使用最新收到的那一封驗證信。',
    resend_failed: '目前無法寄出驗證郵件，請稍後再試。',
    reset_failed: '這個重設連結無效、已過期，或已被較新的重設信取代。請使用最新收到的那一封重設信。',
    change_password_failed: '無法變更密碼，請確認目前的密碼後再試。',
    bind_email_failed: '無法綁定電子郵件。請確認目前密碼，並重新整理帳號的信箱狀態。',
    transfer_failed: '無法完成轉帳，請確認收款人、餘額與電子郵件驗證狀態後再試。',
    authentication_required: '請先登入。',
    invalid_request: '送出的內容格式不正確。',
    rate_limited: '嘗試次數過多，請稍後再試。',
    request_too_large: '送出的內容過大。',
    session_expired: '登入已過期，請重新登入。',
    service_unavailable: '帳號服務暫時無法使用，請稍後再試。',
    request_failed: '操作未完成，請稍後再試。',
  },
  en: {
    credentials_required: 'Enter your username and password.',
    login_failed: 'The username or password is incorrect.',
    registration_failed: 'Could not create the account. Check the username and email, then try again.',
    username_taken: 'This username has already been used.',
    email_in_use: 'This email address has already been used.',
    username_too_short: 'Username must be at least 3 characters.',
    invalid_email: 'Enter a valid email address.',
    password_too_short: 'Password must be at least 8 characters.',
    password_too_long: 'Password must be at most 72 bytes.',
    password_edge_whitespace: 'A password cannot start or end with a space.',
    password_mismatch: 'Passwords do not match.',
    password_required: 'Enter your current password.',
    invalid_recipient: 'Enter the recipient username.',
    invalid_amount: 'Enter a whole number of CPT greater than 0.',
    captcha_required: 'Complete the verification first.',
    captcha_failed: 'Verification failed. Try again.',
    captcha_not_configured: 'Verification is unavailable right now. Try again later.',
    captcha_unavailable: 'Verification is unavailable right now. Try again later.',
    verification_failed: 'This verification link is invalid, has expired, or was replaced by a newer verification email. Use the most recent email.',
    resend_failed: 'Could not send the verification email. Try again later.',
    reset_failed: 'This reset link is invalid, has expired, or was replaced by a newer reset email. Use the most recent email.',
    change_password_failed: 'Could not change the password. Check your current password and try again.',
    bind_email_failed: 'Could not add the email. Check your current password and refresh your account email status.',
    transfer_failed: 'The transfer could not be completed. Check the recipient, your balance, and email verification.',
    authentication_required: 'Sign in first.',
    invalid_request: 'The request was not in the expected format.',
    rate_limited: 'Too many attempts. Try again later.',
    request_too_large: 'The request is too large.',
    session_expired: 'Your session has expired. Sign in again.',
    service_unavailable: 'The account service is unavailable. Try again later.',
    request_failed: 'The request could not be completed. Try again later.',
  },
};

const WELCOME_GRANT_MESSAGES = {
  zh: {
    granted: '歡迎點數已發放。',
    already_granted: '歡迎點數已發放過。',
    pool_empty: '歡迎點數暫時無法發放：發放池的 CPT 不足。',
  },
  en: {
    granted: 'The welcome grant has been credited.',
    already_granted: 'The welcome grant was already credited.',
    pool_empty: 'The welcome grant is on hold: the grant pool does not have enough CPT.',
  },
};

/** Maps an account API error code to a message in the visitor's language. */
export function accountErrorMessage(code, locale) {
  const table = locale === 'zh' ? ACCOUNT_MESSAGES.zh : ACCOUNT_MESSAGES.en;
  return Object.hasOwn(table, code) ? table[code] : table.request_failed;
}

/**
 * The welcome grant line for an account. The grant is paid only after the email is
 * verified, so an unverified account gets that explanation and no grant status.
 * An unrecognised status gets no line.
 */
export function welcomeGrantMessage(status, emailVerified, locale) {
  const zh = locale === 'zh';
  if (!emailVerified) {
    return zh
      ? '歡迎點數會在完成電子郵件驗證後發放。'
      : 'The welcome grant is paid after you verify your email.';
  }
  const table = zh ? WELCOME_GRANT_MESSAGES.zh : WELCOME_GRANT_MESSAGES.en;
  return Object.hasOwn(table, status) ? table[status] : '';
}

export function parseAccountBalance(payload) {
  const raw = payload?.balance ?? payload?.cpt_balance;
  if (raw === undefined || raw === null || raw === "") {
    throw new Error("Account service returned an invalid balance.");
  }

  const balance = Number(raw);
  if (!Number.isFinite(balance)) {
    throw new Error("Account service returned an invalid balance.");
  }
  return balance;
}
