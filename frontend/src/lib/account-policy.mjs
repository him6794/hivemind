const ACCOUNT_MESSAGES = {
  zh: {
    credentials_required: '請輸入使用者名稱與密碼。',
    login_failed: '使用者名稱或密碼不正確。',
    registration_failed: '無法建立帳號，請換一個使用者名稱後再試。',
    password_edge_whitespace: '密碼的開頭或結尾不能有空白。',
    rate_limited: '嘗試次數過多，請稍後再試。',
    request_too_large: '送出的內容過大。',
    session_expired: '登入已過期，請重新登入。',
    service_unavailable: '帳號服務暫時無法使用，請稍後再試。',
    request_failed: '操作未完成，請稍後再試。',
  },
  en: {
    credentials_required: 'Enter your username and password.',
    login_failed: 'The username or password is incorrect.',
    registration_failed: 'Could not create the account. Choose a different username and try again.',
    password_edge_whitespace: 'A password cannot start or end with a space.',
    rate_limited: 'Too many attempts. Try again later.',
    request_too_large: 'The request is too large.',
    session_expired: 'Your session has expired. Sign in again.',
    service_unavailable: 'The account service is unavailable. Try again later.',
    request_failed: 'The request could not be completed. Try again later.',
  },
};

/** Maps an account API error code to a message in the visitor's language. */
export function accountErrorMessage(code, locale) {
  const table = locale === 'zh' ? ACCOUNT_MESSAGES.zh : ACCOUNT_MESSAGES.en;
  return Object.hasOwn(table, code) ? table[code] : table.request_failed;
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
