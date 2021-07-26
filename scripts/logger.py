from datetime import datetime
from time import time

from bus import compute_crc


class Logger(object):
    component_names = {}
    decoders = {}
    ignore_recipients = []
    last_message = time()

    def __init__(self, decoders, names):
        self.decoders = decoders
        self.component_names = names

    def log(self, msg):
        recipient = msg[0]
        cmd = msg[1]

        rest = msg[2:]
        data = rest[:-1]
        crc = msg[-1]

        if recipient in self.ignore_recipients:
            return

        crc_valid = crc == compute_crc(msg[:-1])

        if crc_valid and cmd in self.decoders.keys():
            decoder = self.decoders[cmd]
            op = decoder[0]
            out = decoder[1](data)
        else:
            op = "# {:02X}  ".format(cmd)
            out = self._decode_raw(rest)

        self._log(
            '>', self.component_names[recipient],
            op,
            '✓' if crc_valid else '⨯',
            out,
        )

    def _log(self, *args, **kwargs):
        now = time()
        elapsed = '{0:+f}'.format(now - self.last_message)
        self.last_message = now

        now_printable = datetime.utcfromtimestamp(now).strftime('%H:%M:%S.%f')
        print(now_printable, elapsed, *args, **kwargs)

    def _decode_raw(self, data):
        return fmt_bytes(data)


def log_msg(*args, **kwargs):
    elapsed = kwargs.pop('elapsed', None)
    if elapsed:
        elapsed = '{0:+f}'.format(elapsed)

    
    print(datetime.now().strftime('%H:%M:%S.%f'), elapsed, *args, **kwargs)


def fmt_bytes(bs):
    return ' '.join(['{:02X}'.format(c) for c in bs])


class ComponentNamer(dict):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    def __getitem__(self, id):
        try:
            name = super().__getitem__(id)
        except KeyError:
            name = '??? {:02X}'.format(id)

        return self.pad(name)

    def pad(self, msg):
        max_len = 1 + max(6, min(11, *[len(name) for name in self.values()]))
        msg = msg[0:min(len(msg), max_len)]
        return msg + ' '*(max_len-len(msg))
