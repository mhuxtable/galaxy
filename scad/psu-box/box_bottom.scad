
include <box.scad>
use <box.scad>

$fa = 1;
$fs = 0.4;

difference() {
	union() {
		box_bottom(box_size);
		translate(cable_hole_offset) rotate([0,0,90]) cable_hole(3);
		translate(cable_hole_offset) translate([box_size+wall_thickness,0,0]) rotate([0,0,90]) cable_hole(2);
	}

	translate(cable_hole_offset) rotate([0,0,90]) cable_hole(3,true);
	translate(cable_hole_offset) translate([box_size+wall_thickness,0,0]) rotate([0,0,90]) cable_hole(2,true);
}
