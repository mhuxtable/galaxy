import serial


def compute_crc(*bs):
    crc = 0xaa + sum(*bs)
    while crc > 0xFF:
        crc = (crc >> 8) + (crc & 0xFF)
    return crc


class Component(object):
    def __init__(self, name):
        self.name = name


def galaxy_bus():
    ser = serial.Serial()
    ser.baudrate = 9600
    ser.port = '/dev/ttyUSB0'
    ser.stopbits = serial.STOPBITS_ONE
    ser.bytesize = 8
    ser.parity = serial.PARITY_NONE
    ser.rtscts = 0

    return ser
