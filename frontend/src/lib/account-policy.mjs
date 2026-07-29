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
