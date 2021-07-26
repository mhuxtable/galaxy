#!/usr/bin/env python3

import binascii
from datetime import datetime
import serial
from time import time

from bus import Component, galaxy_bus
from decoders import DECODERS
from logger import ComponentNamer, Logger


def read_data(f):
    while True:
        data = f.read()
        if data != '':
            break
    return data


def read_message(start, f):
    msg = bytearray(start)
    while True:
        before = time()
        data = read_data(f)
        after = time()
        diff = after - before
        if diff > 0.005:
            return msg, data
        msg += data


IGNORED_RECIPIENTS = [
    0x90,  # Prox reader in keypad
]


COMPONENTS = {
    0x10: Component(
        name="Reader",
    ),
    0x11: Component(
        name="Panel",
    ),
}


def main():
    ser = galaxy_bus()

    logger = Logger(decoders=DECODERS, names=ComponentNamer({
        id: component.name
        for id, component in COMPONENTS.items()
    }))
    logger.ignore_recipients = IGNORED_RECIPIENTS

    with ser as s:
        msg, start = read_message(b'', s)
        while True:
            msg, start = read_message(start, s)
            if msg == b'':
                continue

            logger.log(bytes(msg))


if __name__ == '__main__':
    main()
