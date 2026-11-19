#!/usr/bin/env python3
"""Seeds a throwaway `plan` data folder with calendars and events around the
current week, for `make dev-server`. Usage: seed-dev-calendars.py <data-dir>"""

import datetime as dt
import pathlib
import sys

root = pathlib.Path(sys.argv[1]) / "calendars"
today = dt.date.today()
monday = today - dt.timedelta(days=today.weekday())


def day(offset):
    return monday + dt.timedelta(days=offset)


def stamp(d, hm):
    h, m = hm
    return f"{d:%Y%m%d}T{h:02d}{m:02d}00"


def event(uid, summary, start, end=None, *, date=False, rrule=None, exdate=None, location=None, extra=""):
    lines = [f"UID:{uid}", f"DTSTAMP:{dt.datetime.now(dt.timezone.utc):%Y%m%dT%H%M%SZ}", f"SUMMARY:{summary}"]
    if date:
        lines += [f"DTSTART;VALUE=DATE:{start:%Y%m%d}", f"DTEND;VALUE=DATE:{end:%Y%m%d}"]
    else:
        lines += [f"DTSTART;TZID=Europe/Stockholm:{start}", f"DTEND;TZID=Europe/Stockholm:{end}"]
    if rrule:
        lines.append(f"RRULE:{rrule}")
    if exdate:
        lines.append(f"EXDATE;TZID=Europe/Stockholm:{exdate}")
    if location:
        lines.append(f"LOCATION:{location}")
    body = "\r\n".join(lines) + extra
    return f"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//calendar dev seed//EN\r\nBEGIN:VEVENT\r\n{body}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"


calendars = {
    "Personal": ("#83D754", [
        ("dentist", "Dentist", stamp(day(1), (9, 30)), stamp(day(1), (10, 15)), {"location": "Folktandvården"}),
        ("dinner", "Dinner with Anna", stamp(day(3), (19, 0)), stamp(day(3), (21, 30)), {}),
        ("gym", "Gym", stamp(day(0), (7, 0)), stamp(day(0), (8, 0)),
         {"rrule": "FREQ=WEEKLY;BYDAY=MO,TH", "exdate": stamp(day(10), (7, 0))}),
        ("birthday", "Mum's birthday", day(5), day(6), {"date": True}),
        ("trip", "Stockholm trip", day(11), day(14), {"date": True}),
    ]),
    "Work": ("#0088FF", [
        ("standup", "Standup", stamp(day(0), (9, 0)), stamp(day(0), (9, 15)), {"rrule": "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"}),
        ("planning", "Sprint planning", stamp(day(2), (10, 0)), stamp(day(2), (11, 30)), {"location": "Room 4"}),
        ("oneone", "1:1 with Sam", stamp(day(2), (10, 30)), stamp(day(2), (11, 0)), {}),
        ("review", "Design review", stamp(day(4), (14, 0)), stamp(day(4), (15, 0)), {}),
        ("lunch", "Lunch talk", stamp(today, (12, 0)), stamp(today, (13, 0)), {}),
    ]),
    "Blocks": ("#FF8C00", [
        ("deep", "Deep work", stamp(day(1), (13, 0)), stamp(day(1), (16, 0)), {}),
    ]),
}

for name, (color, events) in calendars.items():
    folder = root / name
    folder.mkdir(parents=True, exist_ok=True)
    (folder / ".color").write_text(color)
    for uid, summary, start, end, opts in events:
        (folder / f"{uid}.ics").write_text(event(uid, summary, start, end, **opts))

tasks = root / "tasks"
tasks.mkdir(parents=True, exist_ok=True)
(tasks / "todo.ics").write_text(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:todo\r\nSUMMARY:A task, not an event\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
)
print(f"seeded {root}")
