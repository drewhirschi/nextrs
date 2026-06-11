import { NextResponse } from "next/server";
import { remove } from "@/lib/store";

// DELETE /api/todos/{id}  — mirrors react-todos's [id]/route.rs delete()
export async function DELETE(
  _req: Request,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;
  const ok = remove(Number(id));
  return new NextResponse(null, { status: ok ? 200 : 404 });
}
