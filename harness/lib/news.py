"""Show the last delivered watch digest and record private feedback."""

import argparse
import os
from pathlib import Path
import sqlite3
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import watch_digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--days', type=int, default=7)
    feedback = parser.add_mutually_exclusive_group()
    feedback.add_argument('--noise', metavar='ID')
    feedback.add_argument('--more', metavar='ID')
    parser.add_argument('--feedback', type=Path, default=watch_digest.FEEDBACK)
    args = parser.parse_args()
    if args.days <= 0:
        parser.error('days must be positive')
    path = Path(os.environ.get('THREADS_DB') or Path.home() / '.local/state/threads/threads.db')
    items = []
    if path.exists():
        with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            if db.execute("SELECT 1 FROM sqlite_master WHERE name='watch_digest_items'").fetchone():
                if args.noise or args.more:
                    try:
                        watch_digest.record_feedback(db, args.noise or args.more, 'noise' if args.noise else 'more', args.feedback)
                    except (OSError, ValueError) as error:
                        parser.error(str(error))
                    print('Recorded ' + ('noise' if args.noise else 'more') + ': ' + (args.noise or args.more))
                    return
                items = watch_digest.kept(db, args.days)
    if args.noise or args.more:
        parser.error('no delivered digest item with that ID')
    print('Watch digest: last delivered run')
    if not items:
        print('  none')
    for item in items:
        print(f"[{item['id']}] {item['title']}\n{item['why']}\n{item['url']}")


if __name__ == '__main__':
    main()
