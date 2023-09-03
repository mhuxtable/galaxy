
include <box.scad>
use <box.scad>

$fa = 1;
$fs = 0.4;

difference() {
	union() {
		box_top(box_size);

		translate(cable_hole_offset) rotate([0,0,90]) cable_hole(3);
		translate(cable_hole_offset) translate([box_size+wall_thickness,0,0]) rotate([0,0,90]) cable_hole(2);
	}

	translate(cable_hole_offset) rotate([0,0,90]) cable_hole(3,true);
	translate(cable_hole_offset) translate([box_size+wall_thickness,0,0]) rotate([0,0,90]) cable_hole(2,true);

	translate([44,48,-e]) rotate([0,0,-90]) mirror([0,1,0]) linear_extrude(height=1) {
		text("Galaxy PSU", halign="center", size=8);
		translate([0,-15,0]) text("Live parts!", halign="center", size=8);
	}
}


translate([96/2,96/2,3-e]) linear_extrude(height=1) text("MJH", halign="center", valign="center", size=14);
