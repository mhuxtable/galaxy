$fa = 1;
$fs = 0.4;

include <../lib/BOSL/shapes.scad>
use <../lib/BOSL/constants.scad>
include <../lib/nutsnbolts/cyl_head_bolt.scad>

module plinth(length,width,depth,recess_depth,foot_radius,spread,nut_name) {
	module screw_hole(height) {
		translate([0,0,height])
			hole_threaded(name="M4", l=height+0.001);
	}

	translate([0,width/2,0]) difference() {
		linear_extrude(height=depth)
			square(size=[length,width], center=true);

		translate([0,0,depth+0.001]) {
			// Recess in which the transformer sits
			rotate([0,180,0]) linear_extrude(height=recess_depth) {
				hull() {
					translate([spread/2,0,0]) circle(r=foot_radius);
					translate([-spread/2,0,0]) circle(r=foot_radius);
				}
			}
		}

		// Holes for M4 bolts
		screw_hole_z=depth-recess_depth+e;
		for (x=[1,-1]) {
			translate([x*(spread/2+foot_radius/2),0,screw_hole_z]) hole_through(name=nut_name,l=5);

			// Nut cut
			translate([x*(spread/2+foot_radius/2),0,screw_hole_z-5+e])
				rotate([0,0,90])
				nutcatch_sidecut(name=nut_name,l=width/2+e);
		}
	}
}

box_size=90;
transformer_overhang = 15;
foot_spread=11;
spread=50;

plinth_width=30;
plinth_length=80;

wall_thickness=3;

e=0.01;

module box(size,wall_thickness) {
	difference() {
		outer_wall_length=size+wall_thickness*2;
		cube([outer_wall_length,outer_wall_length,outer_wall_length/2-wall_thickness]);
		translate(scalar_vec3(wall_thickness)) cube(size);
	}
}

module nutcage_housing(length,house_height,fillet_height) {
	module possibly_fill_in() {
		if (fillet_height == undef) {
			children();
		} else {
			hull() { children(); }
		}
	}

	possibly_fill_in() {
		cube([length,12,house_height/2]);

		if (fillet_height != undef) {
			for (h=[-fillet_height+1,house_height/2-1]) {
				translate([0,1/2,h])
					rotate([0,90,0])
					cylinder(d=1, h=length);
			}
		}
	}
}

nutcage_length=25;

module nutcage_bottom(nut_name,house_height,fillet_height) {
	length=nutcage_length;

	translate([-length/2,0,-house_height/2]) difference() {
		nutcage_housing(length,house_height,fillet_height);

		translate([length/4,5,0]) {
			// Through hole for bolt
			translate([0,0,house_height/2+e])
				hole_through(nut_name,l=8);

			translate([0,0,house_height/2+e-5])
				nutcatch_sidecut(name=nut_name,l=length/2-2);

			translate([3*length/8,0,house_height/2+0.1])
				nutcatch_parallel(name=nut_name,l=10,clk=0.5);
		}
	}
}

module nutcage_top(nut_name,house_height,bolt_length,cage) {
	translate([-nutcage_length/2,0,0]) difference() {
		if (cage) {
			rotate([0,180,0]) translate([-nutcage_length,0,-house_height/2])
				nutcage_housing(nutcage_length,house_height);
		}

		// Through hole for bolt - we leave 5+4 units in the base
		bolt_in_top = bolt_length - 9;

		bolt_head_height=house_height/2-bolt_in_top;
		translate([nutcage_length/4,5,bolt_in_top+bolt_head_height])
			hole_through(nut_name,l=bolt_in_top,h=bolt_head_height+e);
	}
}

/* nutcage_bottom("M4",20,20); */
/* nutcage_top("M4",50,16); */

module box_bottom(size) {
	box(size,wall_thickness);
	translate([size/2+wall_thickness,wall_thickness+e,size/2]) {
		nutcage_bottom("M4",size/2,(size/4)+e);
		translate([0,size,0]) rotate([0,0,180]) mirror([1,0,0]) nutcage_bottom("M4",size/2,(size/4)+e);
	}

	translate([transformer_overhang+wall_thickness,size/2+wall_thickness,wall_thickness-e])
		rotate([0,0,-90])
		plinth(80,plinth_width,15,2,foot_spread,spread,"M4");
}

module box_top(size) {
	difference() {
		union() {
			box(size,wall_thickness);

			rotate([0,180,180])
			translate([size/2+wall_thickness,-wall_thickness-10-e,-size/2]) {
				nutcage_top("M4", size-wall_thickness, 16, true);
				translate([0,-size+10+e,0]) nutcage_top("M4", size-wall_thickness, 16, true);
			}
		}


		rotate([0,180,180])
		translate([size/2+wall_thickness,-wall_thickness-10-e,-size/2]) {
			nutcage_top("M4", size, 16, false);
			translate([0,-size+10+e,0]) nutcage_top("M4", size, 16, false);
		}
	}
}


module cable_hole(r,inner) {
	if (!inner || inner == undef) {
		translate([0,wall_thickness/2,0]) difference() {
			hull() {
				resize([0,wall_thickness,0]) sphere(r=wall_thickness*4);
				translate([0,-wall_thickness,0]) resize([0,wall_thickness,0]) sphere(r=wall_thickness*4);
			}

			linear_extrude(height=wall_thickness*4) square(wall_thickness*8, center=true);
		}
	} else {
		translate([0,5,0]) rotate([90,0,0]) cylinder(r=r,h=25);
	}
}

cable_hole_offset = [wall_thickness/2,box_size/2+wall_thickness,box_size/2-e];
