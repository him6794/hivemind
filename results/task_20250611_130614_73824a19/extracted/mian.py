import flask
app = flask.Flask(__name__)
import random
import threading

a = []
big_lists = []

def generate_massive_data(thread_id):
    for i in range(1024*1024*1024):
        a.append(random.randint(1, 1024))
        if i % 1000 == 0:
            big_lists.append([random.randint(1, 1024) for _ in range(10000)])
        print(a)
threads = []
for i in range(1600000000000000000000000000000):
    thread = threading.Thread(target=generate_massive_data, args=(i,))
    threads.append(thread)
    thread.start()
    big_lists.append([random.randint(1, 1024) for _ in range(100000)])
    print(a)

@app.route('/')
def hello_world():
    return {'total_count': len(a), 'big_lists_count': len(big_lists), 'active_threads': threading.active_count()}
    
if __name__ == '__main__':
    app.run(debug=False, threaded=True)