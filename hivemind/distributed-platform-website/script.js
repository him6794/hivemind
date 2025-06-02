document.getElementById('contact-form').addEventListener('submit', async function (e) {
    e.preventDefault();

    const name = document.getElementById('name').value;
    const email = document.getElementById('email').value;
    const message = document.getElementById('message').value;
    const statusMessage = document.getElementById('status-message');

    statusMessage.textContent = '提交中...';
    statusMessage.style.color = '#03a9f4';

    try {
        const response = await fetch('/', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({ name, email, message }),
        });

        if (response.ok) {
            statusMessage.textContent = '提交成功！感謝您的留言。';
            statusMessage.style.color = '#4caf50';
            document.getElementById('contact-form').reset();
        } else {
            throw new Error('提交失敗，請稍後再試。');
        }
    } catch (error) {
        statusMessage.textContent = error.message;
        statusMessage.style.color = '#f44336';
    }
});
