import socket, time, sys

port = int(sys.argv[1])
xml_v4 = b"""<?xml version="1.0" encoding="utf-8"?>
<ProbeMatch>
  <Uuid>11111111-2222-4333-a444-555555555555</Uuid>
  <Types>inquiry</Types>
  <DeviceType>DS-2CD2T47G2-L</DeviceType>
  <DeviceDescription>synthetic</DeviceDescription>
  <DeviceSN>DS-2CD2T47G2-L20210618AAWRL12345678</DeviceSN>
  <CommandPort>8000</CommandPort>
  <HttpPort>80</HttpPort>
  <MAC>a4:14:37:5f:4c:6d</MAC>
  <IPv4Address>192.168.1.64</IPv4Address>
  <IPv4SubnetMask>255.255.255.0</IPv4SubnetMask>
  <IPv4Gateway>192.168.1.1</IPv4Gateway>
  <DHCP>false</DHCP>
  <DSPVersion>V5.7.3 build 210618</DSPVersion>
  <Activated>true</Activated>
  <PasswordResetAbility>true</PasswordResetAbility>
  <AnalogChannelNum>0</AnalogChannelNum>
  <DigitalChannelNum>1</DigitalChannelNum>
  <Support>0x00000000</Support>
</ProbeMatch>"""

xml_v6 = xml_v4.replace(b"<IPv4Address>192.168.1.64</IPv4Address>",
    b"<IPv4Address>192.168.1.64</IPv4Address>\n  <IPv6Address>fe80::21b:2bff:fe3c:4d5e</IPv6Address>\n  <IPv6Masklen>64</IPv6Masklen>")

s4 = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s6 = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
ok4 = ok6 = 0
deadline = time.time() + 4.0
while time.time() < deadline:
    try:
        s4.sendto(xml_v4, ("127.0.0.1", port)); ok4 += 1
    except OSError:
        pass
    try:
        s6.sendto(xml_v6, ("::1", port)); ok6 += 1
    except OSError:
        pass
    time.sleep(0.25)
print("sent v4=%d v6=%d" % (ok4, ok6))
